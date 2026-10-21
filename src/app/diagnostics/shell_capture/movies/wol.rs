//! Production WOL Custom Match checkpoints: Main Menu -> Internet -> Custom
//! Match through the real `0x10E`/`0x10F`/`0xC4`/box handlers, with a
//! controlled WOL service that answers the visit's own requests. Nothing
//! here builds a page; the service events are decoded results, not a
//! transport or backend. The accepted login WRITES the MultiPlayer handle:
//! use an isolated RA2_DIR.

use super::*;
use crate::app::match_runtime::session::SessionMode;
use crate::app::persistence::wol_profile::{WOLINFO_INI, WolInfoFile};
use crate::app::shell_network_setup::{
    self as setup, NetworkSetupEvent, NetworkSetupRequest, WolStartUser,
};
use crate::app::shell_wol::lobby::WolJoined;
use crate::app::shell_wol::{
    WolApiStatus, WolLoginAccepted, WolProvider, WolServer, WolServerList, WolServiceEvent,
    WolServiceRequest, service_event,
};
use crate::ui::shell::geom::RectPx;
use crate::ui::wol_chat::WolChatLine;
use crate::ui::wol_lobby::{
    self as lobby_ui, CHANNEL_KEYED, GAME_CHANNEL, GAME_LIMIT_800, LOBBY_KEY, MEMBER_OPERATOR,
    MEMBER_VOICE, WolChannel, WolMember,
};
use crate::ui::wol_login::{BACK, LOGIN, WolCredential};
use crate::ui::wol_options::WolPreferences;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WolTarget {
    /// `0x10F` settled after a controlled server list, empty fields.
    LoginSteady,
    /// A typed nickname and an eight-character password: Login enabled.
    LoginTyped,
    /// Back on `0x10F`: a fresh `0x10E` settled.
    LoginBack,
    /// The server-list wait box `0xC4` once its 7000 ms timer showed it.
    ServerListWait,
    /// Disconnect on that box: WOL left, a new `0xE2` settled.
    ServerListDisconnect,
    /// A failed server list: WOL left without a box, a new `0xE2` settled.
    ServerListFailure,
    /// Login rejected with `TXT_BADPASS`: the box `0xD0`.
    LoginRejected,
    /// OK on that box: a fresh `0x10E` settled.
    LoginRejectedReturn,
    /// Login accepted: the custom lobby `0x113` settled with its lists.
    LoginAccepted,
    /// The ping sort header pressed: the game list re-sorted.
    LobbySort,
    /// A typed chat line and a received page in the transcript.
    LobbyChat,
    /// Received public, private and operator lines and both actions in
    /// their sink colors; the user's own line is not shown.
    LobbyIncomingChat,
    /// DisplayAsian off, then `/clan` and `/buddy`: the gated lines hidden.
    LobbyFilteredChat,
    /// Three locates: found in a chat lobby, not online, find disabled.
    LobbyFind,
    /// A ranked user with a clan and a locale: the hovered row's help.
    LobbyUserTooltip,
    /// A lost connection in `0x113`: the `TXT_YOURE_DISCON` box.
    LobbyDisconnected,
    /// OK on that box: the runner ends, a fresh `0x10E` settled.
    LobbyDisconnectReturn,
    /// Another user kicked and banned, then this user kicked: the box,
    /// then OK and `TXT_YOULEFT` with no channel.
    LobbyKicked,
    /// A lost connection during a WOL match: one `TXT_WOL_CONNLOST` row.
    IngameConnectionLost,
    /// A page during a WOL match: its two rows, then PageUser opens `0xCF`
    /// prefilled with the pager and a reply is paged; the match runs on.
    IngamePage,
    /// The same, then Cancel ends state 7.
    IngamePageCancel,
    /// The same, then the match left: the runner logs in again first, and
    /// `0x10F` settles.
    AbortRelogin,
    /// A version-mismatched and a full game selected: their refusals.
    JoinAdmission,
    /// Join on a keyed game: the password popup `0xE1`.
    JoinPassword,
    /// Join on an open game: leave, join, the slide, then C9 settled.
    JoinAccepted,
    /// Create Game: the popup `0xC3`.
    CreatePopup,
    /// `0xC3` OK: leave, create, the slide, then C2 settled.
    CreateAccepted,
    /// C2 Back: the chat lobby rejoined and `0x113` settled again.
    SetupBack,
    /// C2 with a joined guest: its row, ping icon and the join's requests.
    SetupGuestJoined,
    /// C2 Start with an unready guest: RequestAcceptance and the host line.
    SetupStartNotReady,
    /// C2 chat: a page echo and an emote.
    SetupChat,
    /// C2 Start with a ready guest (an option edit cleared it once):
    /// StartGame, OnGameStart, the slide, then the first loading frame.
    SetupStart,
    /// C9 Accept: Ready(true) and Accept disabled.
    GuestAccept,
    /// C9 Back: Leave, then the channel-left event rejoins the chat lobby.
    GuestBack,
    /// A started WOL match left through FF/B6: the chat lobby rejoined.
    AbortReturn,
    /// A started WOL match's FF menu after a physical speed-trackbar jump.
    InternetMenu,
    /// FF's Game Controls press opens BBB over the running WOL match.
    GameControls,
    /// `0x113` Back: the channel left and a fresh `0x10E` settled.
    LobbyBack,
    /// The Options page's offline test service: login to a settled 0x113.
    TestServiceLobby,
    /// The same, then Create Game: C2 settled.
    TestServiceCreate,
    /// The same, then Join on the first game: C9 settled.
    TestServiceJoin,
    /// Test-service C2: the stand-in guest joins, picks Korea and colour 3
    /// and accepts; its next `R` asks for the host's colour, a start and
    /// team 7 (`0x7A8260`): Random colour, the start, no team.
    GuestChoices,
    /// The same guest asks to observe, then to play again.
    GuestObserver,
    /// The host kicks the guest: the server's echo prints
    /// `TXT_USER_KICKED` in C2 and its row goes.
    GuestKick,
    /// Controlled C2 Start: OnGameStart opens the verify `0x130` over C2,
    /// node 0 and the guest connect, a peer box click clears it.
    VerifyDialog,
    /// Test-service C2 Start with its guest: `0x130`, then the
    /// negotiation's "Scenarios don't match" box over it.
    ScenariosMismatch,
    /// The same, then OK: the runner ends, the login goes and `0x10E`
    /// settles.
    MismatchOk,
    /// `0x113` Options: WOL Options `0x2BC` settled, help under Pages.
    OptionsSteady,
    /// `0x2BC` Play Music unchecked: the preview queues -3, nothing saved.
    OptionsMusicOff,
    /// Pages and Buddies toggled, then Cancel: the lobby again, applied
    /// unchanged, RA2MD.INI untouched.
    OptionsCancel,
    /// Pages unchecked and Buddies checked, then OK: `[WOnline]` written,
    /// applied, and a reopened `0x2BC` shows them.
    OptionsOk,
    /// Sound Options: `0xD6` settled after a music rail press.
    SoundOptions,
    /// `0xD6` Back: `0x2BC` settled again.
    SoundOptionsBack,
    /// A login with `[Ask] AutoLogin` unanswered: `0x112` settled.
    AutoLoginQuery,
    /// `0x112` Don't Ask, then No: `[Ask] AutoLogin=no`, `0x113` settled.
    AutoLoginDeclined,
    /// AutoLogin names a stored account: the connect to the persona's
    /// server without `0x10F`, then `0x113` settled.
    AutoLoginSilent,
    /// The same connect rejected: the box, OK, then `0x10F` settled with
    /// AutoLogin cleared.
    AutoLoginFailed,
    /// LastValid prefills `0x10F`'s nickname.
    LoginLastValid,
    /// LastValid's persona fills `0x10E`'s profile.
    WelcomeProfile,
    /// Neither the server nor the persona knows the locale: `0xFE` settled.
    LocaleQuery,
    /// A `0xFE` row and OK: SetLocale, the persona saved, `0x113` settled.
    LocaleChosen,
    /// The persona's Country and Color: Create, then C2 settled with them.
    PersonaPicks,
    /// Test service: `0x113` Page, a name and text, Enter: `0xCF` with the
    /// echo, the result and the stand-in's page back.
    PageDialog,
    /// Test service: an unknown name and Tab: `0xCF`'s find status.
    PageFindStatus,
    /// Test service: a page to an unknown name: `TXT_PAGEUSERGONE`.
    PageUserGone,
    /// Test service: C2's Page: `0xCF` over C2 after a page.
    SetupPage,
    /// Test service: a page on `0x10E` after lobby Back: `0x11E` opens.
    PageReceived,
    /// The same, then a reply: its echo and the stand-in's answer.
    PageReceivedReply,
    /// The same, then Back: `0x10E` again.
    PageReceivedBack,
    /// My Information `0x114` with LastValid's stored account and persona.
    MyInfoSteady,
    /// `0x114` with no nickname: `GUI:Default`, unknown stats.
    MyInfoDefault,
    /// A preferred server picked: saved at once.
    MyInfoServer,
    /// A preferred country picked: saved at once.
    MyInfoCountry,
    /// A locale picked: saved with priority 3, unverified.
    MyInfoLocale,
    /// AutoLogin ticked, unticked and ticked again.
    MyInfoAutoLogin,
    /// Forget Account: the row and LastValid gone, Play Buddy still on.
    MyInfoForget,
    /// Back: `0x10E` with the selected nickname's profile.
    MyInfoBack,
    /// Test service: welcome Play Buddy, the login, then `0x116` with its
    /// four located buddies and Tanya's ladder record selected.
    BuddySteady,
    /// `0x116` Add Buddy: `0x115` with a typed name, Add enabled.
    BuddyAddPopup,
    /// The same, then Enter: the new row and its locate (a chat lobby).
    BuddyAdded,
    /// Ghost selected and removed.
    BuddyRemove,
    /// Tanya selected and paged: the modal `0x11E` replying to her.
    BuddyPage,
    /// Join on Boris in his game: C9 settled.
    BuddyJoinGame,
    /// The same, then C9 Back: `0x116` again.
    BuddyJoinGameBack,
    /// Join on Tanya in a chat lobby: `0x113` settled in her lobby.
    BuddyJoinLobby,
    /// Create Game: `0xC3` OK, then C2 settled.
    BuddyCreate,
    /// The same, then C2 Back: `0x116` again.
    BuddyCreateBack,
    /// Back: `0x10E` with mode 0.
    BuddyBack,
    /// `0x113`'s Buddy List, then Back: `0x113` again.
    BuddyFromLobby,
    /// `0x114` Play Buddy, the login, then Back: `0x114` again.
    BuddyFromMyInfo,
    /// Test service: Join on Tanya's game; the guest's first R, then the
    /// host's options: Tanya on row 0 under the host marker.
    GuestHostOptions,
    /// The same, then the host leaves: the guest leaves, C9 closes and
    /// `0x113` settles again, no box.
    GuestHostLeft,
    /// The same, then this guest is kicked: `TXT_YOURE_KICKED` in `0x113`.
    GuestKicked,
    /// The same, then another user joins and is kicked: its row and the
    /// `TXT_USER_KICKED` line in C9.
    GuestUserKicked,
    /// The same, then the host changes to a map this guest cannot verify:
    /// one `P`, re-sent on each options text after 900 buckets; the host's
    /// `Z3` shows "Preview unavailable" in place of the thumbnail.
    GuestNoPreview,
    /// The same change, then the host's offer: the guest acks, takes
    /// `Preview.bin`, and the next options text asks for nothing.
    GuestPreviewReceived,
}

impl WolTarget {
    /// The login is accepted and the custom lobby runs.
    fn lobby(self) -> bool {
        !matches!(
            self,
            Self::LoginSteady
                | Self::LoginTyped
                | Self::LoginBack
                | Self::ServerListWait
                | Self::ServerListDisconnect
                | Self::ServerListFailure
                | Self::LoginRejected
                | Self::LoginRejectedReturn
                | Self::AutoLoginFailed
                | Self::LoginLastValid
                | Self::WelcomeProfile
        ) && !self.my_info()
    }

    /// Welcome's My Information runs `0x114`.
    fn my_info(self) -> bool {
        matches!(
            self,
            Self::MyInfoSteady
                | Self::MyInfoDefault
                | Self::MyInfoServer
                | Self::MyInfoCountry
                | Self::MyInfoLocale
                | Self::MyInfoAutoLogin
                | Self::MyInfoForget
                | Self::MyInfoBack
        )
    }

    /// AutoLogin names the stored account: no `0x10F`.
    fn auto_login(self) -> bool {
        matches!(self, Self::AutoLoginSilent | Self::AutoLoginFailed)
    }

    /// The controlled table and the persona have no locale: `0xFE` asks.
    fn locale_unknown(self) -> bool {
        matches!(self, Self::LocaleQuery | Self::LocaleChosen)
    }

    /// `[Ask] AutoLogin` unanswered: `0x112` follows the login.
    fn asks_auto_login(self) -> bool {
        matches!(self, Self::AutoLoginQuery | Self::AutoLoginDeclined)
    }

    /// The isolated root's `wolinfoMD.ini` before Internet. The other
    /// targets' user has answered `0x112` and has the server's locale.
    fn wolinfo(self) -> String {
        const ANSWERED: &str = "[Ask]\nAutoLogin=no\n";
        const LOCALE: &str = "Locale=2\nLocaleVerified=yes\n";
        match self {
            Self::AutoLoginQuery | Self::AutoLoginDeclined => format!("[tester]\n{LOCALE}"),
            Self::AutoLoginSilent | Self::AutoLoginFailed => format!(
                "[DefaultPersona]\nAutoLogin={NICKNAME}\n{ANSWERED}[tester]\nServer=Westwood\n{LOCALE}"
            ),
            Self::LoginLastValid | Self::WelcomeProfile => format!(
                "[DefaultPersona]\nLastValid={NICKNAME}\n{ANSWERED}[tester]\n{LOCALE}\
                 Rank=17\nPoints=1234\nWins=21\nLosses=9\nDisconnects=1\n\
                 UpdateTime=12:34\nUpdateDate=10/19/26\n"
            ),
            Self::LocaleQuery | Self::LocaleChosen => ANSWERED.into(),
            Self::PersonaPicks => format!("{ANSWERED}[tester]\n{LOCALE}Country=3\nColor=2\n"),
            Self::MyInfoDefault => ANSWERED.into(),
            Self::BuddyFromMyInfo => {
                format!("[DefaultPersona]\nLastValid={NICKNAME}\n{ANSWERED}[tester]\n{LOCALE}")
            }
            target if target.my_info() => format!(
                "[DefaultPersona]\nLastValid={NICKNAME}\n{ANSWERED}[tester]\nServer=Westwood\n\
                 {LOCALE}Country=3\nColor=2\nRank=17\nPoints=1234\nWins=21\nLosses=9\n\
                 Disconnects=1\nMatchByRes=no\nUpdateTime=12:34\nUpdateDate=10/19/26\n"
            ),
            _ => format!("{ANSWERED}[tester]\n{LOCALE}"),
        }
    }

    /// The Buddy List `0x116` runs from the test service.
    fn buddy(self) -> bool {
        matches!(
            self,
            Self::BuddySteady
                | Self::BuddyAddPopup
                | Self::BuddyAdded
                | Self::BuddyRemove
                | Self::BuddyPage
                | Self::BuddyJoinGame
                | Self::BuddyJoinGameBack
                | Self::BuddyJoinLobby
                | Self::BuddyCreate
                | Self::BuddyCreateBack
                | Self::BuddyBack
                | Self::BuddyFromLobby
                | Self::BuddyFromMyInfo
        )
    }

    /// Test service: C9 in Tanya's game with the host's options.
    fn guest_service(self) -> bool {
        matches!(
            self,
            Self::GuestHostOptions
                | Self::GuestHostLeft
                | Self::GuestKicked
                | Self::GuestUserKicked
                | Self::GuestNoPreview
                | Self::GuestPreviewReceived
        )
    }

    /// The production test service answers instead of the capture.
    fn test_service(self) -> bool {
        self.buddy()
            || self.guest_service()
            || matches!(
                self,
                Self::TestServiceLobby
                    | Self::TestServiceCreate
                    | Self::TestServiceJoin
                    | Self::GuestChoices
                    | Self::GuestObserver
                    | Self::GuestKick
                    | Self::ScenariosMismatch
                    | Self::MismatchOk
                    | Self::PageDialog
                    | Self::PageFindStatus
                    | Self::PageUserGone
                    | Self::SetupPage
                    | Self::PageReceived
                    | Self::PageReceivedReply
                    | Self::PageReceivedBack
            )
    }

    /// `0x11E` over `0x10E` after lobby Back.
    fn page_received(self) -> bool {
        matches!(
            self,
            Self::PageReceived | Self::PageReceivedReply | Self::PageReceivedBack
        )
    }

    /// The chat lobby's Join leads to C9 instead of Create to C2.
    fn guest(self) -> bool {
        matches!(
            self,
            Self::JoinAccepted | Self::GuestAccept | Self::GuestBack
        )
    }

    /// C2 with the controlled guest joined.
    fn guest_joined(self) -> bool {
        matches!(
            self,
            Self::SetupGuestJoined
                | Self::SetupStartNotReady
                | Self::SetupChat
                | Self::SetupStart
                | Self::AbortReturn
                | Self::InternetMenu
                | Self::GameControls
                | Self::IngameConnectionLost
                | Self::IngamePage
                | Self::IngamePageCancel
                | Self::AbortRelogin
                | Self::VerifyDialog
        )
    }

    /// The lobby's Options page runs.
    fn options(self) -> bool {
        matches!(
            self,
            Self::OptionsSteady
                | Self::OptionsMusicOff
                | Self::OptionsCancel
                | Self::OptionsOk
                | Self::SoundOptions
                | Self::SoundOptionsBack
        )
    }

    /// The started match runs past loading.
    fn in_match(self) -> bool {
        matches!(
            self,
            Self::AbortReturn
                | Self::InternetMenu
                | Self::GameControls
                | Self::IngameConnectionLost
                | Self::IngamePage
                | Self::IngamePageCancel
                | Self::AbortRelogin
        )
    }
}

pub(crate) const WOL_CHECKPOINTS: &[(&str, WolTarget)] = &[
    ("wol-0x10f-steady", WolTarget::LoginSteady),
    ("wol-0x10f-typed", WolTarget::LoginTyped),
    ("wol-0x10f-back", WolTarget::LoginBack),
    ("wol-0xc4-server-list-wait", WolTarget::ServerListWait),
    ("wol-0xc4-disconnect", WolTarget::ServerListDisconnect),
    ("wol-server-list-failure", WolTarget::ServerListFailure),
    ("wol-0xd0-login-rejected", WolTarget::LoginRejected),
    ("wol-login-rejected-return", WolTarget::LoginRejectedReturn),
    ("wol-0x113-login-accepted", WolTarget::LoginAccepted),
    ("wol-0x113-sort", WolTarget::LobbySort),
    ("wol-0x113-chat", WolTarget::LobbyChat),
    ("wol-0x113-incoming-chat", WolTarget::LobbyIncomingChat),
    ("wol-0x113-filtered-chat", WolTarget::LobbyFilteredChat),
    ("wol-0x113-find", WolTarget::LobbyFind),
    ("wol-0x113-user-tooltip", WolTarget::LobbyUserTooltip),
    ("wol-0x113-disconnected", WolTarget::LobbyDisconnected),
    (
        "wol-0x113-disconnect-return",
        WolTarget::LobbyDisconnectReturn,
    ),
    ("wol-0x113-kicked", WolTarget::LobbyKicked),
    ("wol-ingame-connlost", WolTarget::IngameConnectionLost),
    ("wol-ingame-page", WolTarget::IngamePage),
    ("wol-ingame-page-cancel", WolTarget::IngamePageCancel),
    ("wol-abort-relogin", WolTarget::AbortRelogin),
    ("wol-0x113-join-admission", WolTarget::JoinAdmission),
    ("wol-0xe1-password", WolTarget::JoinPassword),
    ("wol-c9-join-accepted", WolTarget::JoinAccepted),
    ("wol-0xc3-create", WolTarget::CreatePopup),
    ("wol-c2-create-accepted", WolTarget::CreateAccepted),
    ("wol-c2-back", WolTarget::SetupBack),
    ("wol-0x113-back", WolTarget::LobbyBack),
    ("wol-c2-guest-joined", WolTarget::SetupGuestJoined),
    ("wol-c2-start-not-ready", WolTarget::SetupStartNotReady),
    ("wol-c2-chat", WolTarget::SetupChat),
    ("wol-c2-start-loading", WolTarget::SetupStart),
    ("wol-c9-accept", WolTarget::GuestAccept),
    ("wol-c9-back", WolTarget::GuestBack),
    ("wol-abort-return", WolTarget::AbortReturn),
    ("wol-0xff-menu", WolTarget::InternetMenu),
    ("wol-game-controls", WolTarget::GameControls),
    ("wol-test-service-lobby", WolTarget::TestServiceLobby),
    ("wol-test-service-create", WolTarget::TestServiceCreate),
    ("wol-test-service-join", WolTarget::TestServiceJoin),
    ("wol-c2-guest-choices", WolTarget::GuestChoices),
    ("wol-c2-guest-observer", WolTarget::GuestObserver),
    ("wol-c2-kick", WolTarget::GuestKick),
    ("wol-0x130-verify", WolTarget::VerifyDialog),
    ("wol-0x130-scenarios-mismatch", WolTarget::ScenariosMismatch),
    ("wol-0x130-mismatch-ok", WolTarget::MismatchOk),
    ("wol-0x2bc-steady", WolTarget::OptionsSteady),
    ("wol-0x2bc-music-off", WolTarget::OptionsMusicOff),
    ("wol-0x2bc-cancel", WolTarget::OptionsCancel),
    ("wol-0x2bc-ok", WolTarget::OptionsOk),
    ("wol-0xd6-sound-options", WolTarget::SoundOptions),
    ("wol-0xd6-back", WolTarget::SoundOptionsBack),
    ("wol-0x112-query", WolTarget::AutoLoginQuery),
    ("wol-0x112-no-dont-ask", WolTarget::AutoLoginDeclined),
    ("wol-autologin-skips-0x10f", WolTarget::AutoLoginSilent),
    ("wol-autologin-failure-0x10f", WolTarget::AutoLoginFailed),
    ("wol-0x10f-lastvalid", WolTarget::LoginLastValid),
    ("wol-0x10e-profile", WolTarget::WelcomeProfile),
    ("wol-0xfe-locale", WolTarget::LocaleQuery),
    ("wol-0xfe-ok", WolTarget::LocaleChosen),
    ("wol-c2-persona-picks", WolTarget::PersonaPicks),
    ("wol-0xcf-page", WolTarget::PageDialog),
    ("wol-0xcf-find-status", WolTarget::PageFindStatus),
    ("wol-0xcf-user-gone", WolTarget::PageUserGone),
    ("wol-c2-page", WolTarget::SetupPage),
    ("wol-0x11e-page-received", WolTarget::PageReceived),
    ("wol-0x11e-reply", WolTarget::PageReceivedReply),
    ("wol-0x11e-back", WolTarget::PageReceivedBack),
    ("wol-0x114-steady", WolTarget::MyInfoSteady),
    ("wol-0x114-default", WolTarget::MyInfoDefault),
    ("wol-0x114-server-change", WolTarget::MyInfoServer),
    ("wol-0x114-country-change", WolTarget::MyInfoCountry),
    ("wol-0x114-locale-change", WolTarget::MyInfoLocale),
    ("wol-0x114-autologin", WolTarget::MyInfoAutoLogin),
    ("wol-0x114-forget", WolTarget::MyInfoForget),
    ("wol-0x114-back", WolTarget::MyInfoBack),
    ("wol-0x116-steady", WolTarget::BuddySteady),
    ("wol-0x115-add", WolTarget::BuddyAddPopup),
    ("wol-0x116-added", WolTarget::BuddyAdded),
    ("wol-0x116-remove", WolTarget::BuddyRemove),
    ("wol-0x116-page", WolTarget::BuddyPage),
    ("wol-0x116-join-buddy-game", WolTarget::BuddyJoinGame),
    ("wol-0x116-join-game-back", WolTarget::BuddyJoinGameBack),
    ("wol-0x116-join-buddy-lobby", WolTarget::BuddyJoinLobby),
    ("wol-0x116-create", WolTarget::BuddyCreate),
    ("wol-0x116-create-back", WolTarget::BuddyCreateBack),
    ("wol-0x116-back-to-welcome", WolTarget::BuddyBack),
    ("wol-0x116-back-to-lobby", WolTarget::BuddyFromLobby),
    ("wol-0x116-back-to-my-info", WolTarget::BuddyFromMyInfo),
    ("wol-c9-host-options", WolTarget::GuestHostOptions),
    ("wol-c9-host-left", WolTarget::GuestHostLeft),
    ("wol-c9-kicked", WolTarget::GuestKicked),
    ("wol-c9-user-kicked", WolTarget::GuestUserKicked),
    ("wol-c9-nat-no-preview", WolTarget::GuestNoPreview),
    ("wol-c9-preview-received", WolTarget::GuestPreviewReceived),
];

/// The controlled guest of the C2 targets, the host of the C9 targets.
const PEER: &str = "Alpha";
/// The test service's stand-in guest of a created game.
const SERVICE_GUEST: &str = "Natasha";

const NICKNAME: &str = "Tester";
const PASSWORD: &str = "secret12";
const BAD_PASSWORD: u32 = 0x8004_0065;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WolPhase {
    Main,
    /// The test service's start is outstanding, then its mismatch box.
    Mismatch,
    /// The mismatch box's OK ended the runner; `0x10E` is due.
    MismatchClosed,
    /// The host kicked the stand-in guest; the echo is outstanding.
    GuestKicked,
    Welcome,
    /// Custom Match pressed: its teardown, then the API request.
    Custom,
    /// The server list is outstanding.
    Servers,
    Login,
    /// Login or Back pressed on `0x10F`.
    LoginClosing,
    Rejected,
    /// A new `0x10E` after the login flow returned 0.
    WelcomeAgain,
    /// WOL left: a new `0xE2`.
    MainReturn,
    /// The runner's chat-lobby list and first join behind the wait box.
    Lobby,
    /// `0x113` shown: its entry slide runs.
    LobbyShown,
    /// Join or Create asked to leave the chat lobby first.
    Leaving,
    /// The game join was requested and `0x113` slides out.
    Joining,
    /// The create was requested.
    Creating,
    /// C2/C9 opening.
    Setup,
    /// C2/C9 Back: the lobby rejoins.
    Rejoin,
    /// OnGameStart accepted: the slide, then the network loading screen.
    Loading,
    /// The started match runs: B5/B6 input until EXIT is queued.
    AbortInput,
    /// EXIT queued: the match returns to the chat lobby.
    MatchLeaving,
    /// Test service: Custom Match pressed, waiting for 0x10F.
    ServiceLogin,
    /// Test service: Login pressed, waiting for 0x113.
    ServiceLobby,
    /// Test service: 0xC3 OK pressed, waiting for C2.
    ServiceSetup,
    /// `0x113` Options pressed: waiting for `0x2BC`.
    OptionsOpening,
    /// `0x2BC` closed: waiting for the lobby.
    OptionsReturn,
    /// OK's result checked: `0x2BC` opened again.
    OptionsReopened,
    /// Sound Options pressed: waiting for `0xD6`.
    SoundOpening,
    /// `0xD6` Back pressed: waiting for `0x2BC`.
    SoundReturn,
    /// The silent connect rejected: its box.
    AutoLoginBox,
    /// `0x112` or `0xFE` opening.
    PersonaPage,
    /// `0x112` or `0xFE` closed: the login continues.
    PersonaClosing,
    /// `0xCF` input sent: waiting for the test service's answers.
    PageAnswers,
    /// Lobby Back for `0x11E`: waiting for `0x10E`.
    PageWelcome,
    /// `0x11E` open: its answers or its Back.
    PageReceivedAnswers,
    /// My Information pressed: its teardown, then the driver's requests.
    MyInfoOpening,
    /// `0x114` opening: its entry slide runs.
    MyInfoShown,
    /// `0x114` Back pressed: waiting for `0x10E`.
    MyInfoBack,
    /// Test service: `0x114` opening for Play Buddy.
    BuddyMyInfo,
    /// `0x116` opening: its rows fill from the test service.
    BuddyShown,
    /// Add or Remove sent: waiting for the service's list to change and,
    /// after Add, the new row's locate.
    BuddyStored,
    /// `0x116` Create: waiting for `0xC3` over the backdrop.
    BuddyCreatePopup,
    /// `0x116` Join or `0xC3` OK: waiting for C2/C9.
    BuddySetup,
    /// `0x116` Back or Join on a chat lobby: waiting for the next page.
    BuddyReturn,
    Settle(u32),
}

pub(super) struct WolCapture {
    target: WolTarget,
    phase: WolPhase,
    route: Vec<Value>,
    /// Setup Back or the match return rejoined the chat lobby.
    rejoined: bool,
    abort_input: Option<abort_return::AbortReturnInput>,
    exit_consumed: bool,
    /// RA2MD.INI and the live WOL preferences when Options was pressed.
    options_before: Option<(Option<Vec<u8>>, WolPreferences)>,
    /// The locale `0xFE` OK sent, ahead of the runner's requests.
    chosen_locale: Option<i32>,
    /// The sim tick when the in-match `0xCF` sent its page.
    page_tick: Option<u64>,
}

impl WolCapture {
    pub(super) fn new(target: WolTarget) -> Self {
        Self {
            target,
            phase: WolPhase::Main,
            route: Vec::new(),
            rejoined: false,
            abort_input: None,
            exit_consumed: false,
            options_before: None,
            chosen_locale: None,
            page_tick: None,
        }
    }

    pub(super) fn timeout(&self) -> Duration {
        // The abort return loads and runs a retail map first.
        Duration::from_secs(if self.target.in_match() { 120 } else { 60 })
    }

    fn event(&mut self, frame: u32, action: impl Into<Value>) {
        self.route
            .push(json!({"frame": frame, "action": action.into()}));
    }

    fn settle(&mut self) {
        self.phase = WolPhase::Settle(SETTLE_FRAMES);
    }

    fn requests(&mut self, state: &mut AppState, frame: u32) -> Result<Vec<WolServiceRequest>> {
        let requests = state
            .frontend
            .wol
            .as_mut()
            .context("WOL visit")?
            .take_service_requests();
        self.route.push(json!({"frame": frame,
            "requests": requests.iter().map(|r| format!("{r:?}")).collect::<Vec<_>>()}));
        Ok(requests)
    }

    fn page_settled(state: &AppState, kind: ShellSlideKind) -> bool {
        MoviesCapture::slide_settled(state, kind)
            && state.frontend.shell_exit.is_none()
            && state.frontend.shell_page_title.is_terminal()
    }

    pub(super) fn after_present(
        &mut self,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<()> {
        ensure!(
            state.frontend.main_menu_shell_error.is_none(),
            "WOL capture encountered the shell fallback"
        );
        if self.target.in_match()
            && self.phase == WolPhase::Loading
            && state.frontend.screen == GameScreen::Loading
        {
            self.complete_peer_loading(state, frame)?;
        }
        match self.phase {
            WolPhase::Main
                if rendered == PresentedShell::MainMenu
                    && steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(
                        state,
                    ))? =>
            {
                // The Options box, read when Internet opens the visit; the
                // capture never writes the profile out.
                state.persistence.options_profile.wol_test_service = self.target.test_service();
                if self.target == WolTarget::BuddyFromMyInfo {
                    state.frontend.wol_test_credentials = vec![WolCredential {
                        slot: 1,
                        nickname: NICKNAME.into(),
                        password: PASSWORD.into(),
                    }];
                }
                write_wolinfo(state, self.target)?;
                App::leave_shell_dialog(
                    state,
                    crate::app::frontend::shell_transition::ShellExitThen::MainMenu(
                        crate::ui::main_menu_shell::MainMenuShellAction::WwOnline,
                    ),
                );
                self.event(frame, "0xE2 Internet");
                self.phase = WolPhase::Welcome;
            }
            WolPhase::Welcome
                if rendered == PresentedShell::WolWelcome
                    && Self::page_settled(state, ShellSlideKind::WolWelcome) =>
            {
                if self.target == WolTarget::WelcomeProfile {
                    let texts = &state
                        .frontend
                        .wol
                        .as_ref()
                        .context("WOL visit")?
                        .welcome_texts;
                    let text = |id: u16| {
                        texts
                            .iter()
                            .find(|(text_id, _)| *text_id == id)
                            .map(|(_, text)| text.as_str())
                    };
                    ensure!(
                        text(0x06EF).is_some_and(|heading| heading.contains(NICKNAME))
                            && [0x06F4, 0x06F5, 0x06F0, 0x06F1, 0x0701].map(text)
                                == [Some("17"), Some("1234"), Some("21"), Some("9"), Some("1")]
                            && text(0x0797).is_some_and(|line| line.contains("10/19/26")),
                        "LastValid's persona did not fill 0x10E: {texts:?}"
                    );
                    self.event(frame, json!({"0x10E profile": texts}));
                    self.hover_settle(state);
                    return Ok(());
                }
                if self.target.my_info() {
                    state
                        .frontend
                        .wol
                        .as_mut()
                        .context("WOL visit")?
                        .set_provider(WolProvider::Controlled);
                    let rect = App::wol_welcome_layout(state)
                        .page
                        .button_rect(crate::ui::wol_shell::MY_INFORMATION)
                        .context("My Information")?;
                    welcome_click(state, rect);
                    ensure!(
                        state.frontend.shell_exit.is_some(),
                        "My Information did not tear 0x10E down"
                    );
                    self.event(frame, "0x10E My Information (controlled service)");
                    self.phase = WolPhase::MyInfoOpening;
                    return Ok(());
                }
                if self.target.buddy() && self.target != WolTarget::BuddyFromLobby {
                    let (id, next) = if self.target == WolTarget::BuddyFromMyInfo {
                        (crate::ui::wol_shell::MY_INFORMATION, WolPhase::BuddyMyInfo)
                    } else {
                        (crate::ui::wol_shell::PLAY_BUDDY, WolPhase::ServiceLogin)
                    };
                    let rect = App::wol_welcome_layout(state)
                        .page
                        .button_rect(id)
                        .context("welcome button")?;
                    welcome_click(state, rect);
                    self.event(frame, json!({"0x10E button (test service)": id}));
                    self.phase = next;
                    return Ok(());
                }
                if self.target.test_service() {
                    let rect = App::wol_welcome_layout(state)
                        .page
                        .button_rect(crate::ui::wol_shell::CUSTOM_MATCH)
                        .context("Custom Match")?;
                    welcome_click(state, rect);
                    self.event(frame, "0x10E Custom Match (Options test service)");
                    self.phase = WolPhase::ServiceLogin;
                    return Ok(());
                }
                state
                    .frontend
                    .wol
                    .as_mut()
                    .context("WOL visit")?
                    .set_provider(WolProvider::Controlled);
                let rect = App::wol_welcome_layout(state)
                    .page
                    .button_rect(crate::ui::wol_shell::CUSTOM_MATCH)
                    .context("Custom Match")?;
                welcome_click(state, rect);
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "Custom Match did not tear 0x10E down"
                );
                self.event(frame, "0x10E Custom Match (controlled service)");
                self.phase = WolPhase::Custom;
            }
            WolPhase::Custom if state.frontend.shell_exit.is_none() => {
                let requests = self.requests(state, frame)?;
                // After a match WOL_Main applies [WOnline] twice (0x0077B31D,
                // 0x0077B402) before the runner logs in again.
                let applies = if self.exit_consumed { 4 } else { 0 };
                ensure!(
                    requests.len() == applies + 1
                        && requests[..applies].iter().all(|request| matches!(
                            request,
                            WolServiceRequest::SetLangFilter(_)
                                | WolServiceRequest::SetFindPage { .. }
                        ))
                        && requests[applies] == WolServiceRequest::EnsureApi,
                    "the flow did not start with the WOLAPI object: {requests:?}"
                );
                ensure!(
                    state.frontend.wol.as_ref().and_then(|visit| visit.mode())
                        == Some(crate::ui::wol_shell::WolMode::CustomMatch),
                    "Custom Match did not write mode 4"
                );
                service_event(state, WolServiceEvent::Api(WolApiStatus::Available));
                let requests = self.requests(state, frame)?;
                ensure!(
                    requests
                        == [
                            WolServiceRequest::ListCredentials,
                            WolServiceRequest::FetchServers { flags: 0x2900 }
                        ],
                    "the server list was not requested"
                );
                let wait = state
                    .frontend
                    .wol
                    .as_ref()
                    .and_then(|visit| visit.wait_box())
                    .context("no 0xC4 wait box")?;
                ensure!(!wait.shown, "0xC4 must be created hidden");
                self.event(frame, "controlled WOLAPI available");
                match self.target {
                    WolTarget::ServerListWait | WolTarget::ServerListDisconnect => {
                        self.phase = WolPhase::Servers;
                    }
                    WolTarget::ServerListFailure => {
                        service_event(state, WolServiceEvent::ServerList(Err(())));
                        self.event(frame, "controlled server list failure");
                        self.phase = WolPhase::MainReturn;
                    }
                    target if target.auto_login() => {
                        service_event(
                            state,
                            WolServiceEvent::Credentials(vec![WolCredential {
                                slot: 1,
                                nickname: NICKNAME.into(),
                                password: PASSWORD.into(),
                            }]),
                        );
                        service_event(state, WolServiceEvent::NetStatus(0x0004_012F));
                        service_event(state, WolServiceEvent::ServerList(Ok(server_list())));
                        let requests = self.requests(state, frame)?;
                        ensure!(
                            state
                                .frontend
                                .wol
                                .as_ref()
                                .is_some_and(|visit| visit.login().is_none())
                                && requests
                                    == [WolServiceRequest::Connect {
                                        server: server_list().servers[1].clone(),
                                        nickname: NICKNAME.into(),
                                        password: PASSWORD.into(),
                                        changed: false,
                                    }],
                            "AutoLogin did not connect the stored account to the persona's server: {requests:?}"
                        );
                        self.event(frame, "AutoLogin: stored account, persona server Westwood");
                        if target == WolTarget::AutoLoginSilent {
                            service_event(
                                state,
                                WolServiceEvent::Login(Ok(WolLoginAccepted {
                                    motd: "Controlled message of the day".into(),
                                })),
                            );
                            self.event(frame, "controlled login accepted");
                            self.after_login(state, frame)?;
                        } else {
                            service_event(state, WolServiceEvent::Login(Err(BAD_PASSWORD)));
                            self.event(frame, "controlled login rejected 0x80040065");
                            self.phase = WolPhase::AutoLoginBox;
                        }
                    }
                    _ => {
                        service_event(state, WolServiceEvent::Credentials(Vec::new()));
                        service_event(state, WolServiceEvent::NetStatus(0x0004_012F));
                        service_event(state, WolServiceEvent::ServerList(Ok(server_list())));
                        ensure!(
                            state
                                .frontend
                                .wol
                                .as_ref()
                                .is_some_and(|visit| visit.login().is_some()),
                            "the server list did not open 0x10F"
                        );
                        self.event(frame, "controlled server list: 3 records, selected 1");
                        self.phase = WolPhase::Login;
                    }
                }
            }
            WolPhase::Servers => {
                let wait = state
                    .frontend
                    .wol
                    .as_ref()
                    .and_then(|visit| visit.wait_box())
                    .context("the wait box closed")?;
                if !wait.shown {
                    return Ok(());
                }
                let shown_after = wait.opened_at.elapsed().as_millis() as u64;
                self.route
                    .push(json!({"frame": frame, "action": "0xC4 shown",
                    "after_ms": shown_after}));
                if self.target == WolTarget::ServerListWait {
                    MoviesCapture::restore_neutral_pointer(state);
                    self.settle();
                    return Ok(());
                }
                let layout = crate::ui::shell::modal::wait_box_layout(
                    state.renderer.gpu.config.width as i32,
                    state.renderer.gpu.config.height as i32,
                );
                welcome_click(state, layout.disconnect);
                ensure!(state.frontend.wol.is_none(), "Disconnect did not leave WOL");
                self.event(frame, "0xC4 Disconnect");
                self.phase = WolPhase::MainReturn;
            }
            WolPhase::Login
                if rendered == PresentedShell::WolLogin
                    && Self::page_settled(state, ShellSlideKind::WolLogin) =>
            {
                let layout = crate::app::shell_wol::login_layout(state);
                match self.target {
                    WolTarget::LoginSteady => {
                        MoviesCapture::restore_neutral_pointer(state);
                        App::handle_wol_mouse_move(state);
                        self.settle();
                        return Ok(());
                    }
                    // The post-match re-login's 0x10F.
                    WolTarget::AbortRelogin if self.exit_consumed => {
                        MoviesCapture::restore_neutral_pointer(state);
                        App::handle_wol_mouse_move(state);
                        self.settle();
                        return Ok(());
                    }
                    WolTarget::LoginLastValid | WolTarget::AutoLoginFailed => {
                        let login = state
                            .frontend
                            .wol
                            .as_ref()
                            .and_then(|visit| visit.login())
                            .context("0x10F")?;
                        ensure!(
                            login.nickname.display_text() == NICKNAME
                                && info_file(state)?.auto_login().is_empty(),
                            "0x10F did not take the persona nickname with AutoLogin cleared"
                        );
                        self.event(frame, "0x10F settled with the persona nickname");
                        MoviesCapture::restore_neutral_pointer(state);
                        App::handle_wol_mouse_move(state);
                        self.settle();
                        return Ok(());
                    }
                    WolTarget::LoginBack => {
                        login_click(state, button(&layout, BACK)?);
                        self.event(frame, "0x10F Back");
                        self.phase = WolPhase::LoginClosing;
                        return Ok(());
                    }
                    _ => {}
                }
                login_click(state, layout.nickname_face);
                App::handle_wol_key(state, None, true, Some(NICKNAME));
                login_click(state, layout.password);
                App::handle_wol_key(state, None, true, Some(PASSWORD));
                let login = state
                    .frontend
                    .wol
                    .as_ref()
                    .and_then(|visit| visit.login())
                    .context("0x10F")?;
                ensure!(
                    login.login_enabled && login.remember_enabled && !login.forget_enabled,
                    "typed account did not enable Login and Remember"
                );
                self.event(frame, "typed nickname and password");
                if self.target == WolTarget::LoginTyped {
                    MoviesCapture::restore_neutral_pointer(state);
                    App::handle_wol_mouse_move(state);
                    self.settle();
                    return Ok(());
                }
                login_click(state, button(&layout, LOGIN)?);
                self.event(frame, "0x10F Login");
                self.phase = WolPhase::LoginClosing;
            }
            WolPhase::LoginClosing if state.frontend.shell_exit.is_none() => {
                if self.target == WolTarget::LoginBack {
                    self.phase = WolPhase::WelcomeAgain;
                    return Ok(());
                }
                let requests = self.requests(state, frame)?;
                ensure!(
                    requests
                        == [WolServiceRequest::Connect {
                            server: server_list().servers[1].clone(),
                            nickname: NICKNAME.into(),
                            password: PASSWORD.into(),
                            changed: true,
                        }],
                    "Login did not connect to the selected record"
                );
                if self.target.lobby() {
                    service_event(
                        state,
                        WolServiceEvent::Login(Ok(WolLoginAccepted {
                            motd: "Controlled message of the day".into(),
                        })),
                    );
                    self.event(frame, "controlled login accepted");
                    self.after_login(state, frame)?;
                } else {
                    service_event(state, WolServiceEvent::Login(Err(BAD_PASSWORD)));
                    self.event(frame, "controlled login rejected 0x80040065");
                    self.phase = WolPhase::Rejected;
                }
            }
            WolPhase::Rejected => {
                let expected = App::csf_label(state, "TXT_BADPASS", "TXT_BADPASS");
                ensure!(
                    state
                        .frontend
                        .wol
                        .as_ref()
                        .and_then(|visit| visit.message())
                        .is_some_and(|message| message.body == expected),
                    "TXT_BADPASS did not show"
                );
                MoviesCapture::restore_neutral_pointer(state);
                if self.target == WolTarget::LoginRejected {
                    App::handle_wol_mouse_move(state);
                    self.settle();
                    return Ok(());
                }
                let ok = crate::ui::shell::modal::body_ok_layout(
                    state.renderer.gpu.config.width as i32,
                    state.renderer.gpu.config.height as i32,
                )
                .ok;
                welcome_click(state, ok);
                self.event(frame, "0xD0 OK");
                self.phase = WolPhase::WelcomeAgain;
            }
            WolPhase::WelcomeAgain
                if rendered == PresentedShell::WolWelcome
                    && Self::page_settled(state, ShellSlideKind::WolWelcome) =>
            {
                ensure!(
                    state
                        .frontend
                        .wol
                        .as_ref()
                        .and_then(|visit| visit.mode())
                        .is_none(),
                    "the login flow did not return mode 0"
                );
                MoviesCapture::restore_neutral_pointer(state);
                App::handle_wol_mouse_move(state);
                self.event(frame, "fresh 0x10E settled");
                self.settle();
            }
            WolPhase::MainReturn
                if rendered == PresentedShell::MainMenu
                    && steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(
                        state,
                    ))? =>
            {
                ensure!(state.frontend.wol.is_none(), "the WOL visit survived");
                self.event(frame, "new 0xE2 settled");
                self.settle();
            }
            WolPhase::AutoLoginBox => {
                let expected = App::csf_label(state, "TXT_BADPASS", "TXT_BADPASS");
                ensure!(
                    state
                        .frontend
                        .wol
                        .as_ref()
                        .and_then(|visit| visit.message())
                        .is_some_and(|message| message.body == expected),
                    "the silent connect's failure box did not show TXT_BADPASS"
                );
                let ok = crate::ui::shell::modal::body_ok_layout(
                    state.renderer.gpu.config.width as i32,
                    state.renderer.gpu.config.height as i32,
                )
                .ok;
                welcome_click(state, ok);
                self.event(frame, "0xD0 OK");
                self.phase = WolPhase::Login;
            }
            WolPhase::PersonaPage
                if (rendered == PresentedShell::WolAutoLoginQuery
                    && Self::page_settled(state, ShellSlideKind::WolAutoLoginQuery))
                    || (rendered == PresentedShell::WolLocale
                        && Self::page_settled(state, ShellSlideKind::WolLocale)) =>
            {
                self.persona_page(state, frame)?;
            }
            WolPhase::PersonaClosing if state.frontend.shell_exit.is_none() => {
                let file = info_file(state)?;
                if let Some(locale) = self.chosen_locale {
                    let persona =
                        file.load_persona(NICKNAME, &|key| App::csf_label(state, key, key));
                    ensure!(
                        (persona.locale, persona.locale_verified) == (locale, false),
                        "0xFE OK did not save the persona's locale: {persona:?}"
                    );
                } else {
                    ensure!(
                        !file.ask_auto_login(),
                        "Don't Ask and No did not write [Ask] AutoLogin=no"
                    );
                }
                self.event(frame, "persona page closed; wolinfoMD.ini written");
                self.phase = WolPhase::Lobby;
                self.lobby_entry(state, frame)?;
            }
            WolPhase::PageAnswers => self.page_answers(state, frame)?,
            WolPhase::PageWelcome
                if rendered == PresentedShell::WolWelcome
                    && Self::page_settled(state, ShellSlideKind::WolWelcome) =>
            {
                // A current client's page on the welcome: no 0x53F anywhere.
                service_event(
                    state,
                    WolServiceEvent::Page {
                        from: member("Tanya", 0, ""),
                        text: "Are you coming back?".encode_utf16().collect(),
                        ansi: false,
                    },
                );
                let received = crate::app::shell_wol::page::page_received(state)
                    .context("0x11E did not open")?;
                ensure!(
                    received.reply_to == "Tanya" && received.lines.len() == 1,
                    "0x11E did not take the page: {:?}",
                    received.reply_to
                );
                self.event(frame, "page on 0x10E: 0x11E opened");
                let layout = crate::app::shell_wol::page::received_layout(state);
                match self.target {
                    WolTarget::PageReceivedReply => {
                        welcome_click(state, layout.reply);
                        App::handle_wol_key(state, None, true, Some("On my way"));
                        App::handle_wol_key(
                            state,
                            Some(winit::keyboard::KeyCode::Enter),
                            true,
                            None,
                        );
                        self.event(frame, "0x11E reply typed, Enter");
                        self.phase = WolPhase::PageReceivedAnswers;
                    }
                    WolTarget::PageReceivedBack => {
                        welcome_click(state, layout.back);
                        ensure!(
                            crate::app::shell_wol::page::page_received(state).is_none(),
                            "Back did not close 0x11E"
                        );
                        self.event(frame, "0x11E Back");
                        self.hover_settle(state);
                    }
                    _ => self.hover_settle(state),
                }
            }
            WolPhase::PageReceivedAnswers => {
                let received =
                    crate::app::shell_wol::page::page_received(state).context("0x11E closed")?;
                let texts: Vec<String> = received
                    .lines
                    .iter()
                    .map(|line| String::from_utf16_lossy(&line.text))
                    .collect();
                if texts.iter().any(|text| text == "(Tanya) Got your page.") {
                    ensure!(
                        texts.contains(&format!("[{NICKNAME}] On my way"))
                            && received.lines.iter().any(|line| line.color == 0xF0F0),
                        "0x11E did not echo the reply: {texts:?}"
                    );
                    self.event(frame, json!({"0x11E lines": texts}));
                    self.hover_settle(state);
                }
            }
            WolPhase::Lobby => self.lobby_entry(state, frame)?,
            WolPhase::Leaving => {
                let requests = self.requests(state, frame)?;
                if requests.is_empty() {
                    return Ok(());
                }
                // 0x00789B60 calls API+0x24 once unconditionally and once
                // more while joined; 0x00788800 only while joined.
                let leaves = if self.target.guest() { 1 } else { 2 };
                ensure!(
                    requests == vec![WolServiceRequest::LeaveChannel; leaves],
                    "Join/Create did not leave the chat lobby first: {requests:?}"
                );
                service_event(state, WolServiceEvent::ChannelLeft);
                self.event(frame, "controlled channel left");
                let requests = self.requests(state, frame)?;
                if self.target.guest() {
                    ensure!(
                        requests
                            == [WolServiceRequest::JoinChannel {
                                channel: games()[0].clone(),
                                key: String::new(),
                            }],
                        "the open game was not joined"
                    );
                    ensure!(
                        state.frontend.shell_exit.is_some(),
                        "the join did not slide 0x113 out"
                    );
                    self.phase = WolPhase::Joining;
                } else {
                    let [WolServiceRequest::CreateChannel { channel, key }] = requests.as_slice()
                    else {
                        bail!("the game channel was not created: {requests:?}");
                    };
                    ensure!(
                        channel.name == format!("{NICKNAME}'s_game")
                            && channel.kind == GAME_CHANNEL
                            && channel.max_users == 7
                            && key.is_empty(),
                        "unexpected created channel {channel:?}"
                    );
                    self.phase = WolPhase::Creating;
                }
            }
            WolPhase::Joining if state.frontend.shell_exit.is_none() => {
                // The result arrives once 0x113 is hidden.
                let mut channel = games()[0].clone();
                channel.users += 1;
                service_event(
                    state,
                    WolServiceEvent::Joined(Ok(WolJoined {
                        channel,
                        members: vec![
                            member("Alpha", MEMBER_OPERATOR, ""),
                            member(NICKNAME, 0, ""),
                        ],
                    })),
                );
                self.event(frame, "controlled game join accepted");
                self.phase = WolPhase::Setup;
            }
            WolPhase::Creating => {
                service_event(
                    state,
                    WolServiceEvent::Created(Ok(vec![member(NICKNAME, MEMBER_OPERATOR, "")])),
                );
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "the create did not slide 0x113 out"
                );
                self.event(frame, "controlled create accepted");
                self.phase = WolPhase::Setup;
            }
            WolPhase::Setup => {
                let host = !self.target.guest();
                if !(rendered == PresentedShell::NetworkSetup && wol_setup_settled(state, host)) {
                    return Ok(());
                }
                self.event(frame, if host { "C2 settled" } else { "C9 settled" });
                self.setup_settled(state, frame)?;
            }
            WolPhase::Rejoin
                if state.frontend.shell_exit.is_none()
                    && state.frontend.shell_route.wol_lobby() =>
            {
                let mut requests = self.requests(state, frame)?;
                if requests.is_empty() {
                    return Ok(());
                }
                if !self.target.guest() {
                    // C2 Back's own leave (0x7900B8), then the runner's join
                    // (0x788A30) leaves again first (0x788800); the OnPart
                    // says TXT_YOULEFT.
                    ensure!(
                        requests
                            == [
                                WolServiceRequest::LeaveChannel,
                                WolServiceRequest::LeaveChannel
                            ],
                        "C2 Back did not leave the game channel first: {requests:?}"
                    );
                    let game = crate::app::shell_wol::lobby::current_channel(state)
                        .context("the game channel stays until its leave")?;
                    service_event(state, WolServiceEvent::ChannelLeft);
                    let left = crate::assets::csf_file::format_csf(
                        &App::csf_label(state, "TXT_YOULEFT", "TXT_YOULEFT"),
                        &[crate::assets::csf_file::CsfArg::Str(&game.name)],
                    );
                    let shown = Self::lobby(state)?
                        .ui
                        .chat
                        .lines
                        .iter()
                        .any(|line| String::from_utf16_lossy(&line.text) == left);
                    ensure!(shown, "C2 Back's leave did not say {left:?} in 0x113");
                    self.event(frame, json!({"C2 Back left": left}));
                    requests = self.requests(state, frame)?;
                }
                ensure!(
                    requests
                        == [WolServiceRequest::JoinChannel {
                            channel: lobbies()[0].clone(),
                            key: LOBBY_KEY.into(),
                        }],
                    "setup Back did not rejoin the chat lobby: {requests:?}"
                );
                join_lobby(state);
                self.rejoined = true;
                self.event(frame, "controlled chat lobby rejoined");
                self.phase = WolPhase::LobbyShown;
            }
            WolPhase::LobbyShown
                if rendered == PresentedShell::WolLobby
                    && Self::page_settled(state, ShellSlideKind::WolLobby) =>
            {
                self.lobby_settled(state, frame)?;
            }
            WolPhase::Loading
                if self.target.in_match() && state.frontend.screen == GameScreen::InGame =>
            {
                let sim = &state
                    .match_state
                    .sim_runtime
                    .as_ref()
                    .context("installed WOL runtime")?
                    .simulation;
                ensure!(
                    state.match_state.session_mode() == SessionMode::Wol
                        && sim.session().started()
                        && sim.session().game_mode_nonzero(),
                    "the WOL runtime was not admitted"
                );
                // [0xB77DC1]: a line received during the match shows nowhere.
                let lines = |state: &AppState| {
                    state
                        .frontend
                        .wol
                        .as_ref()
                        .and_then(|visit| visit.lobby())
                        .map(|lobby| lobby.ui.chat.lines.len())
                };
                let before = lines(state);
                service_event(
                    state,
                    WolServiceEvent::Message {
                        from: member("Hotel", MEMBER_OPERATOR, ""),
                        text: "mid-match".encode_utf16().collect(),
                        private: false,
                        action: false,
                    },
                );
                ensure!(
                    lines(state) == before,
                    "a line received during the match was shown"
                );
                self.event(frame, "WOL match installed");
                if matches!(
                    self.target,
                    WolTarget::IngamePage | WolTarget::IngamePageCancel
                ) {
                    self.ingame_page(state, frame)?;
                    return Ok(());
                }
                if matches!(
                    self.target,
                    WolTarget::IngameConnectionLost | WolTarget::AbortRelogin
                ) {
                    // NetStatus 0x4012F twice: [0xB77DC2] lets one row through.
                    let rows = |state: &AppState| {
                        state
                            .match_state
                            .match_presentation
                            .message_list
                            .messages()
                            .len()
                    };
                    let before = rows(state);
                    for _ in 0..2 {
                        service_event(
                            state,
                            WolServiceEvent::NetStatus(crate::app::shell_wol::NET_STATUS_LOST),
                        );
                    }
                    let lost = App::csf_label(state, "TXT_WOL_CONNLOST", "TXT_WOL_CONNLOST");
                    let messages = state.match_state.match_presentation.message_list.messages();
                    ensure!(
                        rows(state) == before + 1
                            && messages.last().is_some_and(|row| row.text.contains(&lost)),
                        "the lost connection did not post one TXT_WOL_CONNLOST row"
                    );
                    self.event(frame, "NetStatus 0x4012F twice in the match");
                    if self.target == WolTarget::IngameConnectionLost {
                        self.phase = WolPhase::Settle(2);
                        return Ok(());
                    }
                }
                self.abort_input = Some(match self.target {
                    WolTarget::InternetMenu => {
                        abort_return::AbortReturnInput::begin_menu(state, &mut self.route, frame)?
                    }
                    WolTarget::GameControls => abort_return::AbortReturnInput::begin_game_controls(
                        state,
                        &mut self.route,
                        frame,
                    )?,
                    _ => abort_return::AbortReturnInput::begin(state, &mut self.route, frame)?,
                });
                self.phase = WolPhase::AbortInput;
            }
            WolPhase::AbortInput => {
                if self
                    .abort_input
                    .as_mut()
                    .context("WOL abort input")?
                    .after_present(state, &mut self.route, frame)?
                {
                    self.phase = if matches!(
                        self.target,
                        WolTarget::InternetMenu | WolTarget::GameControls
                    ) {
                        ensure!(
                            crate::app::input::pause_menu::current_template(state)
                                == crate::ui::shell::pause_menu::PauseMenuTemplate::Internet,
                            "a WOL match did not open FF"
                        );
                        WolPhase::Settle(2)
                    } else {
                        WolPhase::MatchLeaving
                    };
                }
            }
            WolPhase::MatchLeaving => {
                if state.match_state.scenario_exit.is_some() && !self.exit_consumed {
                    let sim = &state
                        .match_state
                        .sim_runtime
                        .as_ref()
                        .context("EXIT runtime")?
                        .simulation;
                    ensure!(sim.quit_requested(), "the return began before EXIT ran");
                    self.event(frame, "EXIT consumed");
                    self.exit_consumed = true;
                }
                if self.target == WolTarget::AbortRelogin
                    && state.frontend.shell_route == crate::app::shell_route::ShellRoute::Wol
                    && state.frontend.shell_exit.is_none()
                {
                    ensure!(
                        self.exit_consumed
                            && state
                                .frontend
                                .wol
                                .as_ref()
                                .is_some_and(|visit| visit.lobby().is_none()),
                        "the post-match runner did not log in again first"
                    );
                    self.event(frame, "post-match runner logs in again");
                    self.phase = WolPhase::Custom;
                    return Ok(());
                }
                if state.frontend.shell_route.wol_lobby() && state.frontend.shell_exit.is_none() {
                    ensure!(
                        self.exit_consumed
                            && state.match_state.session_mode() == SessionMode::Wol
                            && state.frontend.loading_session.is_none()
                            && state.match_state.scenario_exit.is_none()
                            && state.frontend.multiplayer_setup.is_none()
                            && state.frontend.network_setup_protocol.is_none(),
                        "the WOL match return cleanup is incomplete"
                    );
                    // A fresh runner, not the resumed one: lobby_entry
                    // requires it hidden behind its wait box.
                    self.event(frame, "post-match 0x113 opened");
                    self.rejoined = true;
                    self.phase = WolPhase::Lobby;
                }
            }
            WolPhase::ServiceLogin
                if rendered == PresentedShell::WolLogin
                    && Self::page_settled(state, ShellSlideKind::WolLogin) =>
            {
                let layout = crate::app::shell_wol::login_layout(state);
                // A stored account selected by LastValid is ready as it is.
                let ready = state
                    .frontend
                    .wol
                    .as_ref()
                    .and_then(|visit| visit.login())
                    .is_some_and(|login| login.login_enabled);
                if !ready {
                    login_click(state, layout.nickname_face);
                    App::handle_wol_key(state, None, true, Some(NICKNAME));
                    login_click(state, layout.password);
                    App::handle_wol_key(state, None, true, Some(PASSWORD));
                }
                login_click(state, button(&layout, LOGIN)?);
                self.event(frame, "0x10F Login (test service)");
                self.phase = if self.target.buddy() && self.target != WolTarget::BuddyFromLobby {
                    WolPhase::BuddyShown
                } else {
                    WolPhase::ServiceLobby
                };
            }
            WolPhase::ServiceLobby
                if rendered == PresentedShell::WolLobby
                    && Self::page_settled(state, ShellSlideKind::WolLobby) =>
            {
                let lobby = Self::lobby(state)?;
                ensure!(
                    lobby.shown
                        && lobby.ui.lobby_rows.len() == 3
                        && lobby.ui.game_rows.len() == 3
                        && lobby.ui.player_rows.iter().any(|row| row.name == NICKNAME),
                    "the test service did not fill 0x113"
                );
                self.event(frame, "0x113 settled from the test service");
                if self.target == WolTarget::BuddyFromLobby && !self.rejoined {
                    let layout = crate::app::shell_wol::lobby::layout(state);
                    lobby_button(state, &layout, lobby_ui::BUDDY)?;
                    self.event(frame, "0x113 Buddy List");
                    self.phase = WolPhase::BuddyShown;
                    return Ok(());
                }
                if matches!(
                    self.target,
                    WolTarget::TestServiceCreate
                        | WolTarget::SetupPage
                        | WolTarget::GuestChoices
                        | WolTarget::GuestObserver
                        | WolTarget::GuestKick
                        | WolTarget::ScenariosMismatch
                        | WolTarget::MismatchOk
                ) {
                    let layout = crate::app::shell_wol::lobby::layout(state);
                    lobby_button(state, &layout, lobby_ui::CREATE)?;
                    let (width, height) = (
                        state.renderer.gpu.config.width as i32,
                        state.renderer.gpu.config.height as i32,
                    );
                    let ok = crate::ui::shell::modal::wol_create_game_layout(width, height).ok;
                    welcome_click(state, ok);
                    self.event(frame, "0xC3 OK (test service)");
                    self.phase = WolPhase::ServiceSetup;
                    return Ok(());
                }
                if matches!(
                    self.target,
                    WolTarget::PageDialog | WolTarget::PageFindStatus | WolTarget::PageUserGone
                ) {
                    let layout = crate::app::shell_wol::lobby::layout(state);
                    lobby_button(state, &layout, lobby_ui::PAGE_BUTTON)?;
                    self.page_input(state, frame, false)?;
                    return Ok(());
                }
                if self.target.page_received() {
                    let layout = crate::app::shell_wol::lobby::layout(state);
                    lobby_button(state, &layout, lobby_ui::BACK)?;
                    self.event(frame, "0x113 Back");
                    self.phase = WolPhase::PageWelcome;
                    return Ok(());
                }
                if self.target == WolTarget::TestServiceJoin || self.target.guest_service() {
                    if self.rejoined {
                        self.event(frame, "0x113 settled after C9 closed");
                        let lobby = Self::lobby(state)?;
                        let lines: Vec<String> = lobby
                            .ui
                            .chat
                            .lines
                            .iter()
                            .map(|line| String::from_utf16_lossy(&line.text))
                            .collect();
                        if self.target == WolTarget::GuestKicked {
                            let kicked =
                                App::csf_label(state, "TXT_YOURE_KICKED", "TXT_YOURE_KICKED");
                            let expected = crate::assets::csf_file::format_csf(
                                &kicked,
                                &[
                                    crate::assets::csf_file::CsfArg::Str("Tanya's_game"),
                                    crate::assets::csf_file::CsfArg::Str("Tanya"),
                                ],
                            );
                            ensure!(
                                lines.contains(&expected),
                                "the kick line did not reach 0x113: {lines:?}"
                            );
                        }
                        ensure!(lobby.message().is_none(), "C9's close showed a box");
                        self.event(frame, json!({"0x113 transcript": lines}));
                        self.hover_settle(state);
                        return Ok(());
                    }
                    game_row_click(state, 0)?;
                    let layout = crate::app::shell_wol::lobby::layout(state);
                    lobby_button(state, &layout, lobby_ui::JOIN)?;
                    self.event(frame, "Join on Tanya's_game (test service)");
                    self.phase = WolPhase::ServiceSetup;
                    return Ok(());
                }
                self.hover_settle(state);
            }
            WolPhase::ServiceSetup
                if rendered == PresentedShell::NetworkSetup
                    && wol_setup_settled(
                        state,
                        self.target != WolTarget::TestServiceJoin && !self.target.guest_service(),
                    ) =>
            {
                ensure!(
                    state
                        .match_state
                        .session
                        .network_setup()
                        .is_some_and(|launch| launch.local_player().name == NICKNAME),
                    "C2/C9 did not open for the local user"
                );
                self.event(frame, "C2/C9 settled from the test service");
                if self.target.guest_service() {
                    return self.guest_options(state, frame);
                }
                if self.target == WolTarget::SetupPage {
                    setup_button(state, crate::ui::multiplayer_setup::PAGE)?;
                    self.page_input(state, frame, true)?;
                    return Ok(());
                }
                if matches!(
                    self.target,
                    WolTarget::GuestChoices
                        | WolTarget::GuestObserver
                        | WolTarget::GuestKick
                        | WolTarget::ScenariosMismatch
                        | WolTarget::MismatchOk
                ) {
                    return self.service_host(state, frame);
                }
                MoviesCapture::restore_neutral_pointer(state);
                crate::app::input::multiplayer_setup::cursor_moved(state);
                self.settle();
            }
            WolPhase::OptionsOpening
                if rendered == PresentedShell::WolOptions
                    && Self::page_settled(state, ShellSlideKind::WolOptions) =>
            {
                self.options_settled(state, frame)?;
            }
            WolPhase::OptionsReturn
                if rendered == PresentedShell::WolLobby
                    && Self::page_settled(state, ShellSlideKind::WolLobby) =>
            {
                self.options_returned(state, frame)?;
            }
            WolPhase::OptionsReopened
                if rendered == PresentedShell::WolOptions
                    && Self::page_settled(state, ShellSlideKind::WolOptions) =>
            {
                let checked = Self::options_page(state)?.ui.checked;
                ensure!(
                    checked == [false, true, true, true, true, true, true, true, false],
                    "the reopened 0x2BC does not show the saved values: {checked:?}"
                );
                self.event(frame, json!({"0x2BC reopened": checked}));
                // The capture's isolated profile goes back as it was.
                let (bytes, _) = self.options_before.clone().context("saved RA2MD.INI")?;
                restore_ra2md(state, bytes)?;
                self.hover_settle(state);
            }
            WolPhase::SoundOpening
                if rendered == PresentedShell::WolSoundOptions
                    && Self::page_settled(state, ShellSlideKind::WolSoundOptions) =>
            {
                self.sound_settled(state, frame)?;
            }
            WolPhase::SoundReturn
                if rendered == PresentedShell::WolOptions
                    && Self::page_settled(state, ShellSlideKind::WolOptions) =>
            {
                ensure!(
                    Self::options_page(state)?.sound.is_none(),
                    "0xD6 survived its Back"
                );
                self.event(frame, "0x2BC settled after 0xD6 Back");
                self.hover_settle(state);
            }
            WolPhase::BuddyMyInfo
                if rendered == PresentedShell::WolMyInformation
                    && Self::page_settled(state, ShellSlideKind::WolMyInformation) =>
            {
                self.buddy_my_info(state, frame)?;
            }
            WolPhase::BuddyShown
                if rendered == PresentedShell::WolBuddyList
                    && Self::page_settled(state, ShellSlideKind::WolBuddyList) =>
            {
                self.buddy_shown(state, frame)?;
            }
            WolPhase::Mismatch => {
                let Some(verify) = crate::app::shell_network_setup::verify(state) else {
                    return Ok(());
                };
                let Some(message) = &verify.mismatch else {
                    return Ok(());
                };
                let expected = App::csf_label(
                    state,
                    "TXT_SCENARIOS_DO_NOT_MATCH",
                    "TXT_SCENARIOS_DO_NOT_MATCH",
                );
                ensure!(
                    message.body == expected
                        && verify.names == [NICKNAME.to_owned(), SERVICE_GUEST.to_owned()]
                        && verify.checked[..2] == [true, true],
                    "the mismatch box over 0x130: {verify:?}"
                );
                self.event(frame, json!({"0x130": verify.names, "box": message.body}));
                if self.target == WolTarget::MismatchOk {
                    let (width, height) = (
                        state.renderer.gpu.config.width as i32,
                        state.renderer.gpu.config.height as i32,
                    );
                    let ok = crate::ui::shell::modal::body_ok_layout(width, height).ok;
                    setup_click(state, ok);
                    self.event(frame, "mismatch box OK");
                    self.phase = WolPhase::MismatchClosed;
                    return Ok(());
                }
                MoviesCapture::restore_neutral_pointer(state);
                crate::app::input::multiplayer_setup::cursor_moved(state);
                self.settle();
            }
            WolPhase::MismatchClosed
                if rendered == PresentedShell::WolWelcome
                    && Self::page_settled(state, ShellSlideKind::WolWelcome) =>
            {
                let visit = state.frontend.wol.as_ref().context("WOL visit")?;
                ensure!(
                    !visit.authenticated()
                        && visit.lobby().is_none()
                        && state.match_state.session.network_setup().is_none(),
                    "the abort did not log out and close the runner"
                );
                self.event(frame, "0x10E settled after the abort, logged out");
                self.hover_settle(state);
            }
            WolPhase::GuestKicked => {
                let gone = state
                    .match_state
                    .session
                    .network_setup()
                    .is_some_and(|launch| {
                        !launch
                            .humans
                            .iter()
                            .any(|human| human.name == SERVICE_GUEST)
                    });
                if !gone {
                    return Ok(());
                }
                let channel = crate::app::shell_wol::lobby::current_channel(state)
                    .context("C2's game channel")?;
                let line = crate::assets::csf_file::format_csf(
                    &App::csf_label(state, "TXT_USER_KICKED", "TXT_USER_KICKED"),
                    &[
                        crate::assets::csf_file::CsfArg::Str(SERVICE_GUEST),
                        crate::assets::csf_file::CsfArg::Str(&channel.name),
                        crate::assets::csf_file::CsfArg::Str(NICKNAME),
                    ],
                );
                ensure!(
                    setup_chat_has(state, &line)?,
                    "the kick echo did not say {line:?} in C2"
                );
                self.event(frame, json!({"kick echo": line}));
                MoviesCapture::restore_neutral_pointer(state);
                crate::app::input::multiplayer_setup::cursor_moved(state);
                self.settle();
            }
            WolPhase::BuddyStored => {
                let listed = |name: &str| {
                    state
                        .frontend
                        .wol_test_buddies
                        .iter()
                        .any(|known| known == name)
                };
                let done = if self.target == WolTarget::BuddyAdded {
                    listed("Natasha") && state.frontend.wol_buddies.status("Natasha") == Some(4)
                } else {
                    !listed("Ghost")
                };
                if done {
                    let rows = Self::buddy_rows(state)?;
                    self.event(frame, json!({"service buddy list": state.frontend.wol_test_buddies, "rows": rows}));
                    self.hover_settle(state);
                }
            }
            WolPhase::BuddyCreatePopup
                if state.frontend.shell_exit.is_none()
                    && Self::lobby(state).is_ok_and(|lobby| lobby.ui.popup.is_some()) =>
            {
                let (width, height) = (
                    state.renderer.gpu.config.width as i32,
                    state.renderer.gpu.config.height as i32,
                );
                let ok = crate::ui::shell::modal::wol_create_game_layout(width, height).ok;
                welcome_click(state, ok);
                self.event(frame, "0xC3 OK over the backdrop");
                self.phase = WolPhase::BuddySetup;
            }
            WolPhase::BuddySetup
                if rendered == PresentedShell::NetworkSetup
                    && wol_setup_settled(state, self.buddy_hosts()) =>
            {
                self.buddy_setup(state, frame)?;
            }
            WolPhase::BuddyReturn if self.buddy_returned(state, rendered) => {
                self.buddy_return(state, frame)?;
            }
            WolPhase::MyInfoOpening if state.frontend.shell_exit.is_none() => {
                self.my_info_open(state, frame)?;
            }
            WolPhase::MyInfoShown
                if rendered == PresentedShell::WolMyInformation
                    && Self::page_settled(state, ShellSlideKind::WolMyInformation) =>
            {
                self.my_info_act(state, frame)?;
            }
            WolPhase::MyInfoBack
                if rendered == PresentedShell::WolWelcome
                    && Self::page_settled(state, ShellSlideKind::WolWelcome) =>
            {
                let texts = &state
                    .frontend
                    .wol
                    .as_ref()
                    .context("WOL visit")?
                    .welcome_texts;
                ensure!(
                    texts
                        .iter()
                        .any(|(id, text)| *id == 0x06EF && text.contains("Alpha")),
                    "0x10E did not show the nickname 0x114 selected: {texts:?}"
                );
                self.event(frame, json!({"0x10E profile": texts}));
                self.hover_settle(state);
            }
            WolPhase::Settle(remaining) if remaining > 0 => {
                self.phase = WolPhase::Settle(remaining - 1);
            }
            _ => {}
        }
        Ok(())
    }

    /// C2/C9 shown and settled: drive the target's interaction.
    fn setup_settled(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::ui::multiplayer_setup as setup_ui;
        match self.target {
            WolTarget::PersonaPicks => {
                let slot = state
                    .match_state
                    .session
                    .network_setup()
                    .context("C2 setup")?
                    .local_slot()
                    .clone();
                ensure!(
                    slot.country == crate::skirmish_launch::LaunchCountry::from_country_index(3)
                        && !slot.country_random
                        && slot.color_index == 2
                        && !slot.color_random,
                    "the host row did not take the persona's Country 3 and Color 2: {slot:?}"
                );
                self.event(frame, "C2 host row: persona Country 3, Color 2");
            }
            WolTarget::SetupBack => {
                setup_button(state, setup_ui::BACK)?;
                self.event(frame, "C2 Back");
                self.phase = WolPhase::Rejoin;
                return Ok(());
            }
            WolTarget::JoinAccepted => {
                // Before the host's 'L' (0x5E8EA0) the user-list sink's nodes
                // (0x7A7110) hold no row, so 0x5EA3E0 marks no host.
                let launch = state
                    .match_state
                    .session
                    .network_setup()
                    .context("C9 setup")?;
                let rows: Vec<(String, Option<u8>)> = launch
                    .humans
                    .iter()
                    .map(|human| (human.name.clone(), human.raw_slot_index))
                    .collect();
                let view = setup_view(state)?;
                let marked = view.row_icons.iter().flatten().any(|icon| icon.host);
                ensure!(
                    rows.contains(&(PEER.to_owned(), None)) && !marked,
                    "C9 before options must hold an unbound host and no host icon: \
                     {rows:?} host icon {marked}"
                );
                self.event(frame, json!({"C9 before options": rows}));
            }
            WolTarget::GuestAccept => {
                setup_button(state, setup_ui::ACCEPT)?;
                let requests = setup_requests(state);
                ensure!(
                    requests.contains(&NetworkSetupRequest::Ready(true))
                        && !setup_view(state)?.enabled(setup_ui::ACCEPT),
                    "C9 Accept did not report readiness: {requests:?}"
                );
                self.event(frame, "C9 Accept");
            }
            WolTarget::GuestBack => {
                setup_button(state, setup_ui::BACK)?;
                let requests = setup_requests(state);
                // The user-list sink's first R (0x7A7A5D) precedes Back's leave.
                ensure!(
                    matches!(
                        requests.as_slice(),
                        [
                            NetworkSetupRequest::PlayerChoices { row: 0, .. },
                            NetworkSetupRequest::Leave
                        ]
                    ) && state.frontend.shell_exit.is_none(),
                    "C9 Back must leave the channel and wait: {requests:?}"
                );
                service_event(state, WolServiceEvent::ChannelLeft);
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "the channel-left event did not slide C9 out"
                );
                self.event(frame, "C9 Back, then controlled channel left");
                self.phase = WolPhase::Rejoin;
                return Ok(());
            }
            target if target.guest_joined() => {
                self.join_guest(state, frame)?;
                match target {
                    WolTarget::SetupStartNotReady => {
                        setup_button(state, setup_ui::START)?;
                        let requests = setup_requests(state);
                        let line =
                            App::csf_label(state, "GUI:HostGameStartHost", "GUI:HostGameStartHost");
                        ensure!(
                            requests == [NetworkSetupRequest::RequestAcceptance]
                                && setup_chat_has(state, &line)?,
                            "Start with an unready guest: {requests:?}"
                        );
                        self.event(frame, "C2 Start refused: guest not ready");
                    }
                    WolTarget::SetupChat => self.setup_chat(state, frame)?,
                    WolTarget::SetupStart
                    | WolTarget::AbortReturn
                    | WolTarget::InternetMenu
                    | WolTarget::GameControls
                    | WolTarget::IngameConnectionLost
                    | WolTarget::IngamePage
                    | WolTarget::IngamePageCancel
                    | WolTarget::AbortRelogin => {
                        self.start(state, frame)?;
                        return Ok(());
                    }
                    WolTarget::VerifyDialog => return self.start(state, frame),
                    _ => {}
                }
            }
            _ => {}
        }
        MoviesCapture::restore_neutral_pointer(state);
        crate::app::input::multiplayer_setup::cursor_moved(state);
        self.settle();
        Ok(())
    }

    /// The host's join sink for the controlled guest, its ping and its
    /// map status.
    fn join_guest(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let launch = state
            .match_state
            .session
            .network_setup()
            .context("C2 setup")?;
        let mut slot = launch.local_slot().clone();
        slot.color_index = (slot.color_index + 1) % 8;
        let row = state
            .match_state
            .session
            .network_observer_row()
            .unwrap_or(8);
        let opened = setup_requests(state);
        self.route.push(json!({"frame": frame,
            "c2_open_requests": opened.iter().map(|r| format!("{r:?}")).collect::<Vec<_>>()}));
        setup::service_event(
            state,
            NetworkSetupEvent::ParticipantJoined(crate::skirmish_launch::SkirmishHumanSlot {
                raw_slot_index: Some(1),
                name: PEER.into(),
                role: Default::default(),
                slot,
            }),
        )?;
        let requests = setup_requests(state);
        ensure!(
            requests
                == [
                    NetworkSetupRequest::ObserverSlot {
                        name: PEER.into(),
                        row
                    },
                    NetworkSetupRequest::PublishRoster,
                    NetworkSetupRequest::PublishOptions,
                    NetworkSetupRequest::RequestPings,
                    NetworkSetupRequest::Advertise,
                ],
            "the host join sink's requests: {requests:?}"
        );
        setup::service_event(
            state,
            NetworkSetupEvent::Ping {
                name: PEER.into(),
                ping: 120,
            },
        )?;
        setup::service_event(
            state,
            NetworkSetupEvent::MapStatus {
                index: 1,
                status: 1,
            },
        )?;
        let view = setup_view(state)?;
        ensure!(
            view.ping_icons[1] == crate::ui::multiplayer_setup::ping_icon(120)
                && view.ping_icons[1].is_some()
                && !state.match_state.session.network_player_ready(1),
            "the guest's row, ping icon or readiness: {:?}",
            view.ping_icons
        );
        self.event(frame, "controlled guest joined, ping 120, map present");
        Ok(())
    }

    /// C2 chat: a page through the CSF command, then an emote.
    fn setup_chat(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let page = App::csf_label(state, "TXT_PAGE_COMMAND", "TXT_PAGE_COMMAND");
        let layout = crate::app::input::multiplayer_setup::layout(state);
        setup_click(state, layout.chat);
        crate::app::input::multiplayer_setup::key_press(
            state,
            None,
            Some(&format!("{page} {PEER} ready?")),
        );
        crate::app::input::multiplayer_setup::key_press(
            state,
            Some(winit::keyboard::KeyCode::Enter),
            None,
        );
        let requests = setup_requests(state);
        ensure!(
            requests
                == [NetworkSetupRequest::WolChat(WolChatLine::Page {
                    name: PEER.encode_utf16().collect(),
                    text: "ready?".encode_utf16().collect(),
                })],
            "the page line was not sent: {requests:?}"
        );
        ensure!(
            setup_chat_has(state, &format!("[{NICKNAME} > {PEER}] ready?"))?,
            "the page echo did not show"
        );
        crate::app::input::multiplayer_setup::key_press(state, None, Some("waves"));
        setup_click(state, layout.emote.context("C2 emote")?);
        let requests = setup_requests(state);
        ensure!(
            requests
                == [NetworkSetupRequest::WolEmote(
                    "waves".encode_utf16().collect()
                )]
                && setup_chat_has(state, &format!("{NICKNAME} waves"))?
                && state
                    .frontend
                    .multiplayer_setup
                    .as_ref()
                    .is_some_and(|ui| ui.chat.edit.units.is_empty()),
            "the emote was not sent or echoed: {requests:?}"
        );
        // C2 in the stack passes the language gate: with both display
        // filters off, the guest's line still shows.
        state.frontend.wol_preferences.display_latin = 0;
        state.frontend.wol_preferences.display_asian = 0;
        service_event(
            state,
            WolServiceEvent::Message {
                from: member(PEER, 0, ""),
                text: "\u{4f60}\u{597d}".encode_utf16().collect(),
                private: false,
                action: false,
            },
        );
        ensure!(
            setup_chat_has(state, &format!("[{PEER}] : \u{4f60}\u{597d}"))?,
            "the guest's line did not show in C2"
        );
        self.event(frame, "C2 page, emote and a received line");
        Ok(())
    }

    /// The guest accepts, an option edit clears it, it accepts again; Start
    /// then requests StartGame and the server's OnGameStart completes it.
    fn start(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::ui::multiplayer_setup as setup_ui;
        let ready = || NetworkSetupEvent::PlayerReady {
            index: 1,
            accepted: true,
        };
        setup::service_event(state, ready())?;
        let layout = crate::app::input::multiplayer_setup::layout(state);
        setup_click(
            state,
            crate::ui::skirmish_shell::checkbox_icon_rect(layout.checkboxes[0], layout.page.scale),
        );
        ensure!(
            !state.match_state.session.network_player_ready(1),
            "a C2 option edit kept the guest's acceptance"
        );
        setup_requests(state);
        setup::service_event(state, ready())?;
        setup_button(state, setup_ui::START)?;
        let requests = setup_requests(state);
        ensure!(
            requests
                == [
                    NetworkSetupRequest::PublishRoster,
                    NetworkSetupRequest::PublishOptions,
                    NetworkSetupRequest::StartGame {
                        users: vec![NICKNAME.into(), PEER.into()]
                    },
                ],
            "C2 Start did not commit: {requests:?}"
        );
        // Back and option edits are ignored while the start is pending.
        let options = |state: &AppState| {
            state
                .match_state
                .session
                .network_setup()
                .map(|launch| launch.options.clone())
        };
        let before = options(state);
        setup_button(state, setup_ui::BACK)?;
        setup_click(
            state,
            crate::ui::skirmish_shell::checkbox_icon_rect(layout.checkboxes[0], layout.page.scale),
        );
        ensure!(
            setup_requests(state).is_empty()
                && state.frontend.shell_exit.is_none()
                && options(state) == before
                && state.match_state.session.network_player_ready(1),
            "Back or an option edit acted on a pending start"
        );
        let user = |name: &str, flags| WolStartUser {
            name: name.into(),
            flags,
        };
        let mut pings = [-1; 8];
        pings[1] = 120;
        // A server list naming someone else rejects the start; Start again
        // requests it anew.
        setup::service_event(
            state,
            NetworkSetupEvent::GameStarted {
                users: vec![user(NICKNAME, 1), user("Bravo", 0)],
                pings,
            },
        )?;
        ensure!(
            setup_requests(state).is_empty() && state.frontend.shell_exit.is_none(),
            "a mismatched OnGameStart started the game"
        );
        setup_button(state, setup_ui::START)?;
        ensure!(
            setup_requests(state).last()
                == Some(&NetworkSetupRequest::StartGame {
                    users: vec![NICKNAME.into(), PEER.into()]
                }),
            "Start after a rejected OnGameStart did not request again"
        );
        // 0x7A7C80 compares lowercased names.
        setup::service_event(
            state,
            NetworkSetupEvent::GameStarted {
                users: vec![
                    user(&NICKNAME.to_uppercase(), 1),
                    user(&PEER.to_lowercase(), 0),
                ],
                pings,
            },
        )?;
        // Start Game Now (0x794BA0) keeps C2 up under its verify 0x130.
        let names = |state: &AppState| {
            setup::verify(state).map(|verify| (verify.names.clone(), verify.checked))
        };
        ensure!(
            setup_requests(state).is_empty()
                && state.frontend.shell_exit.is_none()
                && names(state).is_some_and(|(names, checked)| {
                    names == [NICKNAME.to_owned(), PEER.to_owned()] && checked == [false; 8]
                }),
            "OnGameStart did not open 0x130 over C2: {:?}",
            names(state)
        );
        self.event(frame, "C2 Start, then controlled OnGameStart: 0x130");
        // 0x7A7E03 holds [0xB77E28] = 2: a guest's A0 and K0 are dropped
        // under 0x130 (0x5E3E73).
        setup::player_ready(state, PEER, 0)?;
        setup::map_status(state, PEER, 0)?;
        let session = &state.match_state.session;
        ensure!(
            session.network_player_ready(1)
                && session.network_map_status(1) == Some(1)
                && setup_requests(state).is_empty(),
            "a guest packet changed the setup under 0x130"
        );
        setup::service_event(state, NetworkSetupEvent::VerifyStarted { pairs: true })?;
        setup::service_event(state, NetworkSetupEvent::PeerConnected { node: 1 })?;
        // A click holds node 0's box and clears a peer's (0x7932D7).
        let layout = setup::verify_layout(state);
        for node in [0, 1] {
            setup_click(
                state,
                crate::ui::skirmish_shell::checkbox_icon_rect(
                    layout.boxes[node],
                    crate::ui::shell::geom::ShellScale::RETAIL,
                ),
            );
        }
        let checked = names(state).context("0x130")?.1;
        ensure!(
            checked[..2] == [true, false],
            "0x130 clicks: node 0 held, the peer cleared: {checked:?}"
        );
        self.event(frame, json!({"0x130 after clicks": checked[..2]}));
        if self.target == WolTarget::VerifyDialog {
            MoviesCapture::restore_neutral_pointer(state);
            crate::app::input::multiplayer_setup::cursor_moved(state);
            self.settle();
            return Ok(());
        }
        setup::service_event(state, NetworkSetupEvent::Negotiated { result: 0 })?;
        let requests = setup_requests(state);
        ensure!(
            requests == [NetworkSetupRequest::Leave]
                && state.frontend.shell_exit.is_some()
                && setup::verify(state).is_none(),
            "the negotiation did not close 0x130, leave and slide C2 out: {requests:?}"
        );
        MoviesCapture::restore_neutral_pointer(state);
        self.event(frame, "negotiated: 0x130 closed, channel left");
        self.phase = WolPhase::Loading;
        Ok(())
    }

    /// The runner behind its wait box: the chat-lobby list, the picked
    /// lobby's join, then both lists for the shown dialog.
    fn lobby_entry(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        // The mode-loop pass (0x0077B402) applies the preferences first;
        // after a match the entry apply (0x0077B31D) already has WOLAPI.
        let mut requests = self.requests(state, frame)?;
        if let Some(locale) = self.chosen_locale {
            let sent = [
                WolServiceRequest::SetLocale(locale),
                WolServiceRequest::RequestLocales {
                    names: vec![NICKNAME.into()],
                },
            ];
            ensure!(
                requests.starts_with(&sent),
                "0xFE OK did not send the locale: {requests:?}"
            );
            requests.drain(..sent.len());
        }
        // 0x00787BC8 restarts the buddy mirror before TXT_REQ_CHANLIST: the
        // buddy list, then the game list with automatic pings.
        let [apply @ .., buddies, game_list, list] = requests.as_slice() else {
            bail!("the runner sent too little: {requests:?}");
        };
        let passes = if self.rejoined { 2 } else { 1 };
        ensure!(
            apply.len() == 2 * passes
                && apply.chunks(2).all(|pair| pair == Self::applied(state))
                && *buddies == WolServiceRequest::ListBuddies
                && *game_list
                    == WolServiceRequest::ListChannels {
                        kind: GAME_CHANNEL,
                        flags: 1,
                    }
                && *list == WolServiceRequest::ListChannels { kind: 0, flags: 0 },
            "the runner did not apply and request the chat lobbies: {requests:?}"
        );
        let visit = state.frontend.wol.as_ref().context("WOL visit")?;
        ensure!(
            visit.motd().is_none()
                && visit
                    .lobby()
                    .is_some_and(|lobby| !lobby.shown && lobby.wait.is_some()),
            "0x113 must start hidden behind the wait box with the MOTD consumed"
        );
        service_event(
            state,
            WolServiceEvent::Channels {
                kind: 0,
                channels: lobbies(),
            },
        );
        let requests = self.requests(state, frame)?;
        ensure!(
            requests
                == [WolServiceRequest::JoinChannel {
                    channel: lobbies()[0].clone(),
                    key: LOBBY_KEY.into(),
                }],
            "the picker did not join Lob_41_0: {requests:?}"
        );
        join_lobby(state);
        let requests = self.requests(state, frame)?;
        ensure!(
            requests
                == [
                    WolServiceRequest::ListChannels { kind: 0, flags: 0 },
                    WolServiceRequest::ListChannels {
                        kind: GAME_CHANNEL,
                        flags: 1
                    },
                ],
            "the shown lobby did not request both lists: {requests:?}"
        );
        service_event(
            state,
            WolServiceEvent::Channels {
                kind: 0,
                channels: lobbies(),
            },
        );
        service_event(
            state,
            WolServiceEvent::Channels {
                kind: GAME_CHANNEL,
                channels: games(),
            },
        );
        self.event(frame, "controlled lobbies, join and games");
        self.phase = WolPhase::LobbyShown;
        Ok(())
    }

    fn lobby(state: &AppState) -> Result<&crate::app::shell_wol::lobby::WolLobby> {
        state
            .frontend
            .wol
            .as_ref()
            .and_then(|visit| visit.lobby())
            .context("the lobby runner")
    }

    fn chat_has(state: &AppState, text: &str) -> Result<bool> {
        Ok(Self::chat_color(state, text)?.is_some())
    }

    fn chat_color(state: &AppState, text: &str) -> Result<Option<u32>> {
        Ok(Self::lobby(state)?
            .ui
            .chat
            .lines
            .iter()
            .find(|line| String::from_utf16_lossy(&line.text) == text)
            .map(|line| line.color))
    }

    /// A line typed into the lobby's chat edit and sent with Enter.
    fn type_line(state: &mut AppState, text: &str) {
        App::handle_wol_key(state, None, true, Some(text));
        App::handle_wol_key(state, Some(winit::keyboard::KeyCode::Enter), true, None);
    }

    /// `0x113` shown and settled: drive the target's interaction.
    fn lobby_settled(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let layout = crate::app::shell_wol::lobby::layout(state);
        let (width, height) = (
            state.renderer.gpu.config.width as i32,
            state.renderer.gpu.config.height as i32,
        );
        let joined = App::csf_label(state, "TXT_JOINED_S", "TXT_JOINED_S");
        let base = App::csf_label(state, "TXT_LOB_1", "TXT_LOB_1");
        ensure!(
            Self::chat_has(
                state,
                &crate::assets::csf_file::format_csf(
                    &joined,
                    &[crate::assets::csf_file::CsfArg::Str(&base)]
                )
            )?,
            "TXT_JOINED_S was not printed"
        );
        let lobby = Self::lobby(state)?;
        ensure!(
            lobby.ui.game_rows.len() == 4
                && lobby.ui.player_rows.len() == 4
                && lobby.ui.lobby_rows.len() == 3
                && lobby.ui.lobby_list.selected == Some(0),
            "the lists were not filled"
        );
        let visit = state.frontend.wol.as_ref().context("WOL visit")?;
        ensure!(
            visit.authenticated() && state.frontend.shell_route.wol_lobby(),
            "the accepted login did not enter the custom lobby"
        );
        ensure!(
            state
                .frontend
                .multiplayer_shell_runtime
                .local_participant()
                .name
                == NICKNAME,
            "the nickname did not become the MultiPlayer handle"
        );
        if self.rejoined {
            self.event(frame, "0x113 settled again");
            self.hover_settle(state);
            return Ok(());
        }
        self.event(frame, "0x113 settled");
        if self.target.options() {
            self.options_before = Some((read_ra2md(state)?, state.frontend.wol_preferences));
            lobby_button(state, &layout, lobby_ui::OPTIONS)?;
            ensure!(
                state.frontend.shell_exit.is_some(),
                "Options did not slide 0x113 out"
            );
            self.event(frame, "0x113 Options");
            self.phase = WolPhase::OptionsOpening;
            return Ok(());
        }
        match self.target {
            WolTarget::LoginAccepted => {
                ensure!(
                    Self::chat_has(state, "Controlled message of the day")?,
                    "the MOTD was not printed"
                );
            }
            WolTarget::LobbySort => {
                let rect = layout
                    .control_rect(lobby_ui::SORT_PING)
                    .context("ping sort")?;
                welcome_click(state, rect);
                ensure!(
                    state.frontend.wol_lobby_globals.game_sort != 0
                        && Self::lobby(state)?.ui.game_rows[0].name == "Alpha's_game",
                    "the ping sort did not re-sort"
                );
                self.event(frame, "ping sort");
            }
            WolTarget::LobbyChat => {
                App::handle_wol_key(state, None, true, Some("hello"));
                App::handle_wol_key(state, Some(winit::keyboard::KeyCode::Enter), true, None);
                let requests = self.requests(state, frame)?;
                ensure!(
                    requests
                        == [WolServiceRequest::Chat {
                            line: WolChatLine::Say("hello".encode_utf16().collect()),
                            to: Vec::new(),
                        }],
                    "the chat line was not sent: {requests:?}"
                );
                service_event(
                    state,
                    WolServiceEvent::Page {
                        from: member("Golf", 0, ""),
                        text: "are you there?".encode_utf16().collect(),
                        // An older client's ANSI page: the /reply target.
                        ansi: true,
                    },
                );
                ensure!(
                    Self::chat_has(state, &format!("[{NICKNAME}] hello"))?
                        && Self::chat_has(state, "(Golf) are you there?")?
                        && crate::app::shell_wol::last_pager(state) == Some("Golf"),
                    "the echo or the page did not show"
                );
                service_event(state, WolServiceEvent::UserJoined(member("India", 0, "")));
                service_event(state, WolServiceEvent::UserLeft("Foxtrot".into()));
                let names: Vec<String> = Self::lobby(state)?
                    .ui
                    .player_rows
                    .iter()
                    .map(|row| row.name.clone())
                    .collect();
                ensure!(
                    names == ["Hotel", "Echo", "India", NICKNAME],
                    "the membership did not follow join and part: {names:?}"
                );
                self.event(frame, "chat line and received page");
            }
            WolTarget::LobbyIncomingChat => {
                use crate::ui::wol_chat as chat;
                let lines = [
                    ("Hotel", MEMBER_OPERATOR, false, false, "welcome all"),
                    ("Echo", 0, false, false, "anyone for a game?"),
                    ("Foxtrot", MEMBER_VOICE, true, false, "psst"),
                    ("Hotel", MEMBER_OPERATOR, true, false, "behave"),
                    ("Echo", 0, false, true, "waves"),
                    ("Foxtrot", MEMBER_VOICE, true, true, "nods"),
                    (NICKNAME, 0, false, false, "my own line"),
                ];
                for (name, flags, private, action, text) in lines {
                    service_event(
                        state,
                        WolServiceEvent::Message {
                            from: member(name, flags, ""),
                            text: text.encode_utf16().collect(),
                            private,
                            action,
                        },
                    );
                }
                let expected = [
                    ("[Hotel] : welcome all", chat::OPERATOR_COLOR),
                    ("[Echo] : anyone for a game?", chat::PUBLIC_COLOR),
                    ("Foxtrot : psst", chat::PRIVATE_COLOR),
                    ("Hotel : behave", chat::OPERATOR_COLOR),
                    ("Echo waves", chat::EMOTE_COLOR),
                    ("Foxtrot nods", chat::PRIVATE_EMOTE_COLOR),
                ];
                for (text, color) in expected {
                    ensure!(
                        Self::chat_color(state, text)? == Some(color),
                        "{text:?} did not show in {color:#X}"
                    );
                }
                ensure!(
                    !Self::chat_has(state, &format!("[{NICKNAME}] : my own line"))?,
                    "the user's own line was shown"
                );
                self.event(frame, "received lines in their sink colors");
            }
            WolTarget::LobbyFilteredChat => {
                let say = |state: &mut AppState, name, tag, text: &str| {
                    let flags = if name == "Hotel" { MEMBER_OPERATOR } else { 0 };
                    service_event(
                        state,
                        WolServiceEvent::Message {
                            from: member(name, flags, tag),
                            text: text.encode_utf16().collect(),
                            private: false,
                            action: false,
                        },
                    );
                };
                // DisplayAsian off: non-Latin-1 text shows only from an
                // operator.
                state.frontend.wol_preferences.display_asian = 0;
                say(state, "Echo", "WW", "\u{4f60}\u{597d}");
                say(state, "Hotel", "", "\u{4f60}\u{597d}");
                say(state, "Foxtrot", "", "caf\u{e9}");
                ensure!(
                    !Self::chat_has(state, "[Echo] : \u{4f60}\u{597d}")?
                        && Self::chat_has(state, "[Hotel] : \u{4f60}\u{597d}")?
                        && Self::chat_has(state, "[Foxtrot] : caf\u{e9}")?,
                    "the language gate did not hold"
                );
                // /clan: the local user has no clan, so only clanless users
                // show; Echo's clan hides it.
                let clan = App::csf_label(state, "TXT_CLAN_FILTER", "TXT_CLAN_FILTER");
                Self::type_line(state, &clan);
                ensure!(
                    state.frontend.wol_preferences.display_clan,
                    "/clan did not turn DisplayClan on"
                );
                say(state, "Echo", "WW", "clan one");
                say(state, "Foxtrot", "", "clan two");
                ensure!(
                    !Self::chat_has(state, "[Echo] : clan one")?
                        && Self::chat_has(state, "[Foxtrot] : clan two")?,
                    "the clan gate did not hold"
                );
                // /clan off and /buddy on: no buddies, every sender hidden.
                Self::type_line(state, &clan);
                let buddy = App::csf_label(state, "TXT_BUDDY_FILTER", "TXT_BUDDY_FILTER");
                Self::type_line(state, &buddy);
                say(state, "Foxtrot", "", "buddy one");
                say(state, "Hotel", "", "buddy two");
                ensure!(
                    !Self::chat_has(state, "[Foxtrot] : buddy one")?
                        && !Self::chat_has(state, "[Hotel] : buddy two")?,
                    "the buddy gate let a sender through"
                );
                self.event(frame, "language, clan and buddy gates");
            }
            WolTarget::LobbyFind => {
                let online = App::csf_label(state, "TXT_ONLINE_COMMAND", "TXT_ONLINE_COMMAND");
                let room = App::csf_label(state, "TXT_LOB_1", "TXT_LOB_1");
                use crate::assets::csf_file::{CsfArg, format_csf};
                let cases: [(&str, Result<WolChannel, u32>, String); 3] = [
                    (
                        "Hotel",
                        Ok(lobbies()[0].clone()),
                        format_csf(
                            &App::csf_label(state, "TXT_USER_FOUND", "TXT_USER_FOUND"),
                            &[CsfArg::Str("Hotel"), CsfArg::Str(&room)],
                        ),
                    ),
                    (
                        "Zulu",
                        Err(0x0004_0130),
                        format_csf(
                            &App::csf_label(state, "TXT_USER_NOT_ONLINE", "TXT_USER_NOT_ONLINE"),
                            &[CsfArg::Str("Zulu")],
                        ),
                    ),
                    (
                        "Kilo",
                        Err(0x0004_0132),
                        format_csf(
                            &App::csf_label(
                                state,
                                "TXT_USER_FIND_DISABLED",
                                "TXT_USER_FIND_DISABLED",
                            ),
                            &[CsfArg::Str("Kilo")],
                        ),
                    ),
                ];
                for (name, result, expected) in cases {
                    Self::type_line(state, &format!("{online} {name}"));
                    let requests = self.requests(state, frame)?;
                    ensure!(
                        requests
                            == [WolServiceRequest::Chat {
                                line: WolChatLine::Locate {
                                    name: name.encode_utf16().collect(),
                                },
                                to: Vec::new(),
                            }],
                        "the locate was not sent: {requests:?}"
                    );
                    service_event(state, WolServiceEvent::Located(result));
                    ensure!(
                        Self::chat_color(state, &expected)? == Some(u32::MAX),
                        "{expected:?} did not show"
                    );
                }
                self.event(frame, "found, not online and find disabled");
            }
            WolTarget::LobbyUserTooltip => {
                service_event(
                    state,
                    WolServiceEvent::Ladder(vec![crate::app::shell_wol::WolLadderRecord {
                        name: "ECHO".into(),
                        rank: 42,
                        ..Default::default()
                    }]),
                );
                service_event(
                    state,
                    WolServiceEvent::UserLocales(Ok(vec![("Echo".into(), 2)])),
                );
                // Player rows: Hotel, Echo, Foxtrot, the user.
                let lobby = Self::lobby(state)?;
                ensure!(
                    lobby.ui.player_rows.get(1).map(|row| row.name.as_str()) == Some("Echo"),
                    "Echo is not the second player row"
                );
                let geometry = lobby.ui.player_list.geometry(
                    layout.players,
                    lobby.ui.player_rows.len(),
                    layout.page.scale,
                );
                let rect = geometry.row(1 - lobby.ui.player_list.top_index);
                set_window_pointer_to_rect_center(state, rect);
                App::handle_wol_mouse_move(state);
                let expected = format!(
                    "Echo : {} 42: Westwood Warriors : {}",
                    App::csf_label(state, "TXT_RANK", "TXT_RANK"),
                    App::csf_label(state, "WOL:Locale02", "WOL:Locale02")
                );
                let help = crate::app::shell_wol::lobby::status_help(state);
                ensure!(help == expected, "the user tooltip read {help:?}");
                self.event(frame, json!({"hovered user help": help}));
                self.settle();
                return Ok(());
            }
            WolTarget::LobbyDisconnected | WolTarget::LobbyDisconnectReturn => {
                service_event(
                    state,
                    WolServiceEvent::NetStatus(crate::app::shell_wol::NET_STATUS_LOST),
                );
                let expected = App::csf_label(state, "TXT_YOURE_DISCON", "TXT_YOURE_DISCON");
                ensure!(
                    state
                        .frontend
                        .wol
                        .as_ref()
                        .and_then(|visit| visit.message())
                        .is_some_and(|message| message.body == expected),
                    "TXT_YOURE_DISCON did not show"
                );
                self.event(frame, "NetStatus 0x4012F in 0x113");
                if self.target == WolTarget::LobbyDisconnectReturn {
                    let ok = crate::ui::shell::modal::body_ok_layout(width, height).ok;
                    welcome_click(state, ok);
                    ensure!(
                        state
                            .frontend
                            .wol
                            .as_ref()
                            .is_some_and(|visit| visit.lobby().is_none()),
                        "OK did not end the runner"
                    );
                    self.event(frame, "0xD0 OK");
                    self.phase = WolPhase::WelcomeAgain;
                    return Ok(());
                }
            }
            WolTarget::LobbyKicked => {
                use crate::assets::csf_file::{CsfArg, format_csf};
                let channel = lobbies()[0].clone();
                service_event(
                    state,
                    WolServiceEvent::Kicked {
                        channel: channel.clone(),
                        user: member("Foxtrot", MEMBER_VOICE, ""),
                        by: member("Hotel", MEMBER_OPERATOR, ""),
                    },
                );
                service_event(
                    state,
                    WolServiceEvent::ChannelBan {
                        name: "Foxtrot".into(),
                        banned: true,
                    },
                );
                let other = format_csf(
                    &App::csf_label(state, "TXT_USER_KICKED", "TXT_USER_KICKED"),
                    &[
                        CsfArg::Str("Foxtrot"),
                        CsfArg::Str(&channel.name),
                        CsfArg::Str("Hotel"),
                    ],
                );
                let ban = format_csf(
                    &App::csf_label(state, "TXT_CHAN_BAN", "TXT_CHAN_BAN"),
                    &[CsfArg::Str("Foxtrot")],
                );
                ensure!(
                    Self::chat_has(state, &other)?
                        && Self::chat_has(state, &ban)?
                        && !Self::lobby(state)?
                            .ui
                            .player_rows
                            .iter()
                            .any(|row| row.name == "Foxtrot"),
                    "another user's kick or the ban did not show"
                );
                let number = state.frontend.wol_lobby_globals.lobby_number;
                service_event(
                    state,
                    WolServiceEvent::Kicked {
                        channel: channel.clone(),
                        user: member(NICKNAME, lobby_ui::MEMBER_LOCAL, ""),
                        by: member("Hotel", MEMBER_OPERATOR, ""),
                    },
                );
                let kicked = format_csf(
                    &App::csf_label(state, "TXT_YOURE_KICKED", "TXT_YOURE_KICKED"),
                    &[CsfArg::Str(&channel.name), CsfArg::Str("Hotel")],
                );
                ensure!(
                    state
                        .frontend
                        .wol
                        .as_ref()
                        .and_then(|visit| visit.message())
                        .is_some_and(|message| message.body == kicked),
                    "TXT_YOURE_KICKED did not show"
                );
                let ok = crate::ui::shell::modal::body_ok_layout(width, height).ok;
                welcome_click(state, ok);
                let left = format_csf(
                    &App::csf_label(state, "TXT_YOULEFT", "TXT_YOULEFT"),
                    &[CsfArg::Str(&App::csf_label(
                        state,
                        "TXT_LOB_1",
                        "TXT_LOB_1",
                    ))],
                );
                ensure!(
                    Self::chat_has(state, &left)?
                        && Self::lobby(state)?.ui.player_rows.is_empty()
                        && state.frontend.wol_lobby_globals.lobby_number == number + 1
                        && !crate::app::shell_wol::lobby::in_channel(state),
                    "the kick's box did not leave the channel"
                );
                self.event(frame, "kicked, then OK: TXT_YOULEFT and no channel");
            }
            WolTarget::JoinAdmission => {
                // Rows: Alpha, Charlie (version 24), Delta, Bravo (full).
                game_row_click(state, 1)?;
                game_row_click(state, 3)?;
                let mismatch = App::csf_label(state, "TXT_MISMATCH", "TXT_MISMATCH");
                let full = App::csf_label(state, "WOL:GameFull", "WOL:GameFull");
                ensure!(
                    Self::chat_has(state, &mismatch)? && Self::chat_has(state, &full)?,
                    "the admission refusals were not printed"
                );
                ensure!(
                    !Self::lobby(state)?.ui.enabled(lobby_ui::JOIN),
                    "a refused game enabled Join"
                );
                self.event(frame, "mismatched and full games refused");
            }
            WolTarget::JoinPassword => {
                game_row_click(state, 2)?;
                lobby_button(state, &layout, lobby_ui::JOIN)?;
                ensure!(
                    Self::lobby(state)?
                        .ui
                        .popup
                        .as_ref()
                        .map(|popup| popup.dialog())
                        == Some(lobby_ui::PASSWORD_DIALOG),
                    "Join on a keyed game did not open 0xE1"
                );
                self.event(frame, "0xE1 opened");
            }
            WolTarget::JoinAccepted | WolTarget::GuestAccept | WolTarget::GuestBack => {
                game_row_click(state, 0)?;
                lobby_button(state, &layout, lobby_ui::JOIN)?;
                self.event(frame, "Join on Alpha's_game");
                self.phase = WolPhase::Leaving;
                return Ok(());
            }
            target
                if matches!(
                    target,
                    WolTarget::CreatePopup
                        | WolTarget::CreateAccepted
                        | WolTarget::SetupBack
                        | WolTarget::PersonaPicks
                ) || target.guest_joined() =>
            {
                lobby_button(state, &layout, lobby_ui::CREATE)?;
                ensure!(
                    Self::lobby(state)?
                        .ui
                        .popup
                        .as_ref()
                        .map(|popup| popup.dialog())
                        == Some(lobby_ui::CREATE_DIALOG),
                    "Create did not open 0xC3"
                );
                self.event(frame, "0xC3 opened");
                if self.target != WolTarget::CreatePopup {
                    let ok = crate::ui::shell::modal::wol_create_game_layout(width, height).ok;
                    welcome_click(state, ok);
                    self.event(frame, "0xC3 OK");
                    self.phase = WolPhase::Leaving;
                    return Ok(());
                }
            }
            WolTarget::LobbyBack => {
                lobby_button(state, &layout, lobby_ui::BACK)?;
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "Back did not slide 0x113 out"
                );
                self.event(frame, "0x113 Back");
                self.phase = WolPhase::WelcomeAgain;
                return Ok(());
            }
            _ => {}
        }
        self.hover_settle(state);
        Ok(())
    }

    fn options_page(state: &AppState) -> Result<&crate::app::shell_wol::options::WolOptionsPage> {
        match &state.frontend.wol.as_ref().context("WOL visit")?.page {
            crate::app::shell_wol::WolPage::Options(page) => Ok(page),
            _ => bail!("0x2BC is not open"),
        }
    }

    /// The preference requests `0x0077E110` sends with the WOLAPI object.
    fn applied(state: &AppState) -> [WolServiceRequest; 2] {
        let live = state.frontend.wol_preferences;
        [
            WolServiceRequest::SetLangFilter(live.lang_filter),
            WolServiceRequest::SetFindPage {
                find: live.allow_find,
                page: live.allow_page,
            },
        ]
    }

    /// `0x2BC` shown and settled over the hidden lobby.
    fn options_settled(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::ui::wol_options::{
            BUDDIES, CANCEL, MUSIC, OK, PAGES, SOUND_OPTIONS, WolOptionsState,
        };
        let live = state.frontend.wol_preferences;
        let page = Self::options_page(state)?;
        ensure!(
            page.ui.checked == WolOptionsState::open(&live).checked && page.sound.is_none(),
            "0x2BC init does not show the live values"
        );
        ensure!(!Self::lobby(state)?.shown, "0x113 must hide under 0x2BC");
        self.event(frame, json!({"0x2BC settled": page.ui.checked}));
        let checked = |state: &AppState, id| {
            Self::options_page(state).is_ok_and(|page| page.ui.is_checked(id))
        };
        match self.target {
            WolTarget::OptionsSteady => {
                ensure!(
                    live == WolPreferences::default(),
                    "the isolated RA2MD.INI holds [WOnline] values: {live:?}"
                );
                let rect = crate::app::shell_wol::options::layout(state)
                    .checkbox_rect(PAGES)
                    .context("Pages")?;
                set_window_pointer_to_rect_center(state, rect);
                App::handle_wol_mouse_move(state);
                ensure!(
                    crate::app::shell_wol::options::status_key(state) == Some("STT:WOLOptionsPage"),
                    "Pages has no help"
                );
                self.settle();
            }
            WolTarget::OptionsMusicOff => {
                options_checkbox(state, MUSIC)?;
                ensure!(!checked(state, MUSIC), "Play Music stayed checked");
                ensure!(
                    state.audio.theme.saved_slots()[1] == crate::audio::theme::THEME_HOLD,
                    "the preview did not queue -3"
                );
                ensure!(
                    state.frontend.wol_preferences == live
                        && read_ra2md(state)? == self.options_before.clone().context("before")?.0,
                    "the preview changed a preference"
                );
                self.event(frame, "Play Music unchecked: Queue(-3)");
                self.hover_settle(state);
            }
            WolTarget::OptionsCancel | WolTarget::OptionsOk => {
                ensure!(
                    live == WolPreferences::default(),
                    "the isolated RA2MD.INI holds [WOnline] values: {live:?}"
                );
                options_checkbox(state, PAGES)?;
                options_checkbox(state, BUDDIES)?;
                ensure!(
                    !checked(state, PAGES) && checked(state, BUDDIES),
                    "Pages/Buddies did not toggle"
                );
                let id = if self.target == WolTarget::OptionsOk {
                    OK
                } else {
                    CANCEL
                };
                options_button(state, id)?;
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "0x2BC did not slide out"
                );
                self.event(frame, format!("Pages off, Buddies on, button {id:#x}"));
                self.phase = WolPhase::OptionsReturn;
            }
            WolTarget::SoundOptions | WolTarget::SoundOptionsBack => {
                options_button(state, SOUND_OPTIONS)?;
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "0x2BC did not slide out for Sound Options"
                );
                self.event(frame, "0x2BC Sound Options");
                self.phase = WolPhase::SoundOpening;
            }
            _ => bail!("not an Options target"),
        }
        Ok(())
    }

    /// The lobby shown again after `0x2BC` closed.
    fn options_returned(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let lobby = Self::lobby(state)?;
        ensure!(
            lobby.shown
                && matches!(
                    state.frontend.wol.as_ref().map(|visit| &visit.page),
                    Some(crate::app::shell_wol::WolPage::Lobby)
                ),
            "0x113 did not come back"
        );
        let requests = self.requests(state, frame)?;
        ensure!(
            requests == Self::applied(state),
            "0x0077E110 did not send the preferences: {requests:?}"
        );
        let (bytes, before) = self.options_before.clone().context("before")?;
        let live = state.frontend.wol_preferences;
        if self.target == WolTarget::OptionsCancel {
            ensure!(
                live == before
                    && state.persistence.options_profile.wol_preferences == before
                    && read_ra2md(state)? == bytes,
                "Cancel changed the preferences"
            );
            self.event(frame, "0x113 again: Cancel applied the unchanged values");
            self.hover_settle(state);
            return Ok(());
        }
        let expected = WolPreferences {
            allow_page: 0,
            display_buddies: true,
            ..WolPreferences::default()
        };
        ensure!(
            live == expected && state.persistence.options_profile.wol_preferences == expected,
            "OK did not commit the staged values: {live:?}"
        );
        let saved = read_ra2md(state)?.context("RA2MD.INI after OK")?;
        let text = String::from_utf8_lossy(&saved).into_owned();
        let ini = crate::rules::ini_parser::IniFile::from_bytes(&saved)
            .context("parse RA2MD.INI after OK")?;
        let wonline = ini.section("WOnline").context("saved [WOnline]")?;
        let keys = [
            ("AllowPage", "0"),
            ("AllowFind", "1"),
            ("LangFilter", "1"),
            ("LobMusic", "1"),
            ("ShowAll", "1"),
            ("DisplayAsian", "1"),
            ("DisplayLatin", "1"),
            ("DisplayBuddies", "yes"),
            ("DisplayClan", "no"),
        ];
        for (key, value) in keys {
            ensure!(
                wonline.read_string(key, "", 0x40) == value,
                "saved WOnline.{key} is not {value}"
            );
        }
        let at: Vec<usize> = keys
            .iter()
            .map(|(key, _)| text.find(&format!("\n{key}=")).unwrap_or(usize::MAX))
            .collect();
        ensure!(
            at.windows(2).all(|pair| pair[0] < pair[1]),
            "[WOnline] is not in the writer's order"
        );
        let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
        ensure!(
            ini.section_or_empty("MultiPlayer")
                .is_present("ObserverMode")
                && ini
                    .section_or_empty("MultiPlayer")
                    .read_int("ObserverMode", 0)
                    == i32::from(observer),
            "ObserverMode was not written with the preferences"
        );
        self.event(
            frame,
            json!({"0x113 again after OK": text
            .lines()
            .skip_while(|line| line.trim() != "[WOnline]")
            .take(12)
            .collect::<Vec<_>>()}),
        );
        let layout = crate::app::shell_wol::lobby::layout(state);
        lobby_button(state, &layout, lobby_ui::OPTIONS)?;
        self.phase = WolPhase::OptionsReopened;
        Ok(())
    }

    /// `0xD6` shown and settled over the hidden `0x2BC`.
    fn sound_settled(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let positions = crate::app::input::sound::volume_positions(state);
        let dialog = Self::options_page(state)?
            .sound
            .as_ref()
            .context("0xD6 state")?;
        ensure!(dialog.positions == positions, "0xD6 init positions differ");
        // A press on the music rail near its left end jumps once.
        let rail = crate::app::shell_wol::options::sound_layout(state).sliders[0];
        welcome_click(state, RectPx::new(rail.x + 8, rail.y, 4, rail.h));
        let moved = Self::options_page(state)?
            .sound
            .as_ref()
            .context("0xD6 state")?
            .positions;
        ensure!(
            moved[0] < positions[0] && moved[1..] == positions[1..],
            "the music rail press did not move it: {positions:?} -> {moved:?}"
        );
        self.event(
            frame,
            json!({"0xD6 settled": positions, "after rail press": moved}),
        );
        if self.target == WolTarget::SoundOptionsBack {
            let back = crate::app::shell_wol::options::sound_layout(state).back();
            welcome_click(state, back);
            ensure!(
                state.frontend.shell_exit.is_some(),
                "0xD6 Back did not slide out"
            );
            self.event(frame, "0xD6 Back");
            self.phase = WolPhase::SoundReturn;
            return Ok(());
        }
        self.hover_settle(state);
        Ok(())
    }

    /// `0x007864C0`'s continuation after an accepted login: the locale
    /// request answered (2, or unknown for the locale targets), then
    /// `0xFE`, `0x112` or the runner.
    fn after_login(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let requests = self.requests(state, frame)?;
        ensure!(
            requests
                == [WolServiceRequest::RequestLocales {
                    names: vec![NICKNAME.into()],
                }],
            "the login did not request the user's locale: {requests:?}"
        );
        let locale = if self.target.locale_unknown() { 0 } else { 2 };
        service_event(
            state,
            WolServiceEvent::UserLocales(Ok(vec![(NICKNAME.into(), locale)])),
        );
        self.event(frame, format!("controlled locale {locale}"));
        self.phase = if self.target.locale_unknown() {
            let requests = self.requests(state, frame)?;
            ensure!(
                requests == [WolServiceRequest::LocaleCount],
                "0xFE did not ask for the locale count: {requests:?}"
            );
            service_event(state, WolServiceEvent::LocaleCount(38));
            WolPhase::PersonaPage
        } else if self.target.asks_auto_login() {
            WolPhase::PersonaPage
        } else {
            WolPhase::Lobby
        };
        Ok(())
    }

    /// `0x112` or `0xFE` settled: drive the target's answer.
    fn persona_page(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::app::shell_wol::WolPage;
        use crate::ui::wol_persona::{ACCEPT, DECLINE};
        let layout = crate::app::shell_wol::persona::layout(state).context("persona layout")?;
        match self.target {
            WolTarget::AutoLoginQuery | WolTarget::LocaleQuery => {
                self.event(frame, "persona page settled");
                self.hover_settle(state);
            }
            WolTarget::AutoLoginDeclined => {
                welcome_click(
                    state,
                    crate::ui::skirmish_shell::checkbox_icon_rect(
                        layout.control,
                        layout.page.scale,
                    ),
                );
                welcome_click(state, layout.page.button_rect(DECLINE).context("0x112 No")?);
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "Don't Ask and No did not close 0x112"
                );
                self.event(frame, "0x112 Don't Ask, No");
                self.phase = WolPhase::PersonaClosing;
            }
            WolTarget::LocaleChosen => {
                let Some(WolPage::Locale(page)) =
                    state.frontend.wol.as_ref().map(|visit| &visit.page)
                else {
                    bail!("0xFE is not open");
                };
                let geometry = crate::app::shell_wol::persona::list_geometry(&layout, page);
                let locale = page.rows.get(2).context("0xFE rows")?.locale;
                welcome_click(state, geometry.row(2));
                welcome_click(state, layout.page.button_rect(ACCEPT).context("0xFE OK")?);
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "a row and OK did not close 0xFE"
                );
                self.event(frame, json!({"0xFE OK": locale}));
                self.chosen_locale = Some(locale);
                self.phase = WolPhase::PersonaClosing;
            }
            target => bail!("{target:?} has no persona page"),
        }
        Ok(())
    }

    /// `0xCF` opened over `0x113` or C2: the target's name and text through
    /// the real input routes.
    /// The in-match page path: an ANSI page posts its two rows and arms the
    /// reply window; PageUser (U) inside it opens `0xCF` with the pager and
    /// the caret in the text; a reply goes out and the stand-in answers.
    fn ingame_page(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::app::shell_wol::page;
        use crate::ui::pause_menu::InGameMenuState;
        let rows = |state: &AppState| -> Vec<String> {
            state
                .match_state
                .match_presentation
                .message_list
                .messages()
                .iter()
                .map(|row| row.text.clone())
                .collect()
        };
        let before = rows(state).len();
        service_event(
            state,
            WolServiceEvent::Page {
                from: member("Tanya", MEMBER_VOICE, ""),
                text: "where are you?".encode_utf16().collect(),
                ansi: true,
            },
        );
        let hint = App::csf_label(state, "TXT_HOW_TO_REPLY", "TXT_HOW_TO_REPLY");
        let after = rows(state);
        ensure!(
            after.len() == before + 2
                && after[before].contains("(Tanya) where are you?")
                && after[before + 1].contains(&hint),
            "the in-match page did not post its two rows: {after:?}"
        );
        self.event(frame, json!({"in-match page rows": &after[before..]}));
        // U as the window handler hands it to the in-game key edge.
        let u = winit::keyboard::Key::Character("u".into());
        crate::app::input::keyboard::in_game_key_edge(
            state,
            crate::app::input::keyboard::InGameKeyEdge {
                physical: winit::keyboard::KeyCode::KeyU,
                logical: &u,
                unmodified: &u,
                location: winit::keyboard::KeyLocation::Standard,
                state: winit::event::ElementState::Pressed,
                repeat: false,
                text: Some("u"),
            },
            false,
        );
        let opened = page::page_dialog(state).is_some_and(|page| {
            page.name.narrow_text(usize::MAX) == "Tanya"
                && page.focus == Some(crate::ui::wol_page::TEXT)
        });
        ensure!(
            state.match_state.match_presentation.in_game_menu == InGameMenuState::Page && opened,
            "PageUser did not open 0xCF with the pager prefilled"
        );
        self.event(frame, "PageUser opened 0xCF, prefilled");
        page::key_press(state, None, Some("on my way"));
        page::key_press(state, Some(winit::keyboard::KeyCode::Enter), None);
        self.page_tick = state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|runtime| runtime.simulation.session().tick());
        // The controlled service answers the request it was sent.
        let requests = self.requests(state, frame)?;
        let paged = requests.iter().any(|request| {
            matches!(request, WolServiceRequest::Chat {
                line: WolChatLine::Page { name, text },
                ..
            } if String::from_utf16_lossy(name) == "Tanya"
                && String::from_utf16_lossy(text) == "on my way")
        });
        ensure!(paged, "the in-match 0xCF sent no page: {requests:?}");
        self.event(
            frame,
            json!({"in-match 0xCF paged at tick": self.page_tick}),
        );
        service_event(state, WolServiceEvent::PageResult(Ok(())));
        service_event(
            state,
            WolServiceEvent::Page {
                from: member("Tanya", MEMBER_VOICE, ""),
                text: "Got your page.".encode_utf16().collect(),
                ansi: false,
            },
        );
        self.ingame_page_answers(state, frame)
    }

    /// The stand-in's answers in the in-match `0xCF`: the echo, the result
    /// and its UTF-16 page back, which posts one row with no reply hint.
    fn ingame_page_answers(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::app::shell_wol::page;
        use crate::ui::pause_menu::InGameMenuState;
        let texts: Vec<String> = page::page_dialog(state)
            .context("the in-match 0xCF closed")?
            .lines
            .iter()
            .map(|line| String::from_utf16_lossy(&line.text))
            .collect();
        let sent = App::csf_label(state, "TXT_PAGESENT", "TXT_PAGESENT");
        ensure!(
            texts.contains(&format!("[{NICKNAME}] on my way"))
                && texts.contains(&sent)
                && texts.iter().any(|text| text == "(Tanya) Got your page."),
            "the in-match 0xCF lacks the echo or the result: {texts:?}"
        );
        let rows: Vec<String> = state
            .match_state
            .match_presentation
            .message_list
            .messages()
            .iter()
            .map(|row| row.text.clone())
            .collect();
        let hint = App::csf_label(state, "TXT_HOW_TO_REPLY", "TXT_HOW_TO_REPLY");
        ensure!(
            rows.last()
                .is_some_and(|row| row.contains("(Tanya) Got your page."))
                && rows.iter().filter(|row| row.contains(&hint)).count() == 1,
            "the answer did not post one row without a hint: {rows:?}"
        );
        self.event(frame, json!({"in-match 0xCF lines": texts, "rows": rows}));
        if self.target == WolTarget::IngamePageCancel {
            let cancel = page::page_layout(state).cancel;
            state.match_state.input.cursor_x = (cancel.x + cancel.w / 2) as f32;
            state.match_state.input.cursor_y = (cancel.y + cancel.h / 2) as f32;
            for pressed in [
                winit::event::ElementState::Pressed,
                winit::event::ElementState::Released,
            ] {
                crate::app::input::dispatch::handle_mouse_input(
                    state,
                    winit::event::MouseButton::Left,
                    pressed,
                );
            }
            ensure!(
                state.match_state.match_presentation.in_game_menu == InGameMenuState::Closed
                    && page::page_dialog(state).is_none(),
                "Cancel did not end state 7"
            );
            self.event(frame, "Cancel ended state 7");
        }
        self.phase = WolPhase::Settle(SETTLE_FRAMES);
        Ok(())
    }

    fn page_input(&mut self, state: &mut AppState, frame: u32, setup: bool) -> Result<()> {
        use crate::app::shell_wol::page;
        ensure!(page::page_dialog(state).is_some(), "Page did not open 0xCF");
        self.event(frame, "0xCF opened");
        let layout = page::page_layout(state);
        let click = |state: &mut AppState, rect: RectPx| {
            if setup {
                setup_click(state, rect);
            } else {
                welcome_click(state, rect);
            }
        };
        let key =
            |state: &mut AppState, code: Option<winit::keyboard::KeyCode>, text: Option<&str>| {
                if setup {
                    crate::app::input::multiplayer_setup::key_press(state, code, text);
                } else {
                    App::handle_wol_key(state, code, true, text);
                }
            };
        let (name, text) = match self.target {
            WolTarget::SetupPage => ("Boris", Some("Join my game")),
            WolTarget::PageFindStatus => ("Nobody", None),
            WolTarget::PageUserGone => ("Nobody", Some("hello")),
            _ => ("Tanya", Some("hello")),
        };
        click(state, layout.name);
        key(state, None, Some(name));
        match text {
            Some(text) => {
                // Leaving the name for the text locates it.
                click(state, layout.text);
                key(state, None, Some(text));
                ensure!(
                    page::page_dialog(state).is_some_and(|page| page.send_enabled),
                    "0xCF Page stayed disabled"
                );
                key(state, Some(winit::keyboard::KeyCode::Enter), None);
                self.event(frame, json!({"0xCF page": [name, text]}));
            }
            None => {
                key(state, Some(winit::keyboard::KeyCode::Tab), None);
                self.event(frame, json!({"0xCF Tab from": name}));
            }
        }
        self.phase = WolPhase::PageAnswers;
        Ok(())
    }

    /// The test service has answered `0xCF`'s locate and page.
    fn page_answers(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let Some(page) = crate::app::shell_wol::page::page_dialog(state) else {
            bail!("0xCF closed");
        };
        let texts: Vec<String> = page
            .lines
            .iter()
            .map(|line| String::from_utf16_lossy(&line.text))
            .collect();
        let csf = |key: &str| App::csf_label(state, key, key);
        let done = match self.target {
            WolTarget::PageFindStatus => page.status == csf("TXT_FINDPAGE_NOT_ONLINE"),
            WolTarget::PageUserGone => texts.contains(&csf("TXT_PAGEUSERGONE")),
            _ => texts.iter().any(|text| text.ends_with(") Got your page.")),
        };
        if !done {
            return Ok(());
        }
        if matches!(self.target, WolTarget::PageDialog | WolTarget::SetupPage) {
            let name = if self.target == WolTarget::SetupPage {
                "Boris"
            } else {
                "Tanya"
            };
            let echo = texts
                .iter()
                .any(|text| text.starts_with(&format!("[{NICKNAME}] ")));
            ensure!(
                echo && texts.contains(&csf("TXT_PAGESENT")) && !page.status.is_empty(),
                "0xCF lacks the echo, the result or the status: {texts:?} {:?}",
                page.status
            );
            let line = format!("({name}) Got your page.");
            let transcript = if self.target == WolTarget::SetupPage {
                setup_chat_has(state, &line)?
            } else {
                Self::chat_has(state, &line)?
            };
            ensure!(
                transcript,
                "the page back did not reach the transcript below"
            );
        }
        self.event(
            frame,
            json!({"0xCF lines": texts, "status": page.status.clone()}),
        );
        MoviesCapture::restore_neutral_pointer(state);
        if self.target == WolTarget::SetupPage {
            crate::app::input::multiplayer_setup::cursor_moved(state);
            self.settle();
        } else {
            self.hover_settle(state);
        }
        Ok(())
    }

    fn hover_settle(&mut self, state: &mut AppState) {
        MoviesCapture::restore_neutral_pointer(state);
        App::handle_wol_mouse_move(state);
        self.settle();
    }

    /// Once the local loader reports 100, the controlled guest finishes too.
    fn complete_peer_loading(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::app::match_runtime::session::NetworkLoadingRequest;
        let requests = state.match_state.session.take_network_loading_requests();
        if !requests
            .iter()
            .any(|request| matches!(request, NetworkLoadingRequest::ReportProgress(100)))
        {
            return Ok(());
        }
        ensure!(
            crate::app::loading::pump::accept_network_loading_progress(state, 1, 100),
            "controlled guest completion was refused"
        );
        self.route.push(json!({"frame": frame,
            "action": "controlled guest loading completion after the local report",
            "requests": requests.iter().map(|r| format!("{r:?}")).collect::<Vec<_>>()}));
        Ok(())
    }

    pub(super) fn owns_simulation_steps(&self, state: &AppState) -> bool {
        self.target.in_match()
            && self.phase == WolPhase::Loading
            && state.frontend.screen == GameScreen::InGame
    }

    pub(super) fn ready(&self, state: &AppState) -> Result<bool> {
        if self.phase == WolPhase::Loading {
            if self.target.in_match() {
                return Ok(false);
            }
            ensure!(
                state.match_state.session_mode() == SessionMode::Wol,
                "the WOL handoff became offline"
            );
            return Ok(state.frontend.screen == GameScreen::Loading
                && crate::app::loading::pump::next_native_loading_frame(state)
                    == Some(crate::app::loading::pump::NextLoadingFrame::First));
        }
        // Behind a box `0x10E`/`0x10F` is gone, and with it the status line;
        // a hidden or covered `0x113` paints none either, nor does C2/C9's
        // family line belong to this check.
        let backdrop = state.frontend.wol.as_ref().is_some_and(|visit| {
            matches!(
                visit.page,
                crate::app::shell_wol::WolPage::Backdrop { .. }
                    | crate::app::shell_wol::WolPage::Message { .. }
            ) || visit
                .lobby()
                .is_some_and(|lobby| !lobby.shown || lobby.covered || lobby.ui.popup.is_some())
        }) || state.frontend.shell_route.network_setup();
        if let Some(menu) = match self.target {
            WolTarget::InternetMenu => Some(crate::ui::pause_menu::InGameMenuState::Menu),
            WolTarget::IngameConnectionLost | WolTarget::IngamePageCancel => {
                Some(crate::ui::pause_menu::InGameMenuState::Closed)
            }
            WolTarget::IngamePage => Some(crate::ui::pause_menu::InGameMenuState::Page),
            WolTarget::GameControls => Some(crate::ui::pause_menu::InGameMenuState::Options),
            _ => None,
        } {
            // The connection-lost row types itself out on the wall clock.
            let revealed = state
                .match_state
                .match_presentation
                .message_list
                .messages()
                .iter()
                .all(|message| {
                    // Past the text and its eight-unit highlight.
                    let count = message.reveal_count() as usize;
                    count == 0 || count > message.text.encode_utf16().count() + 8
                });
            // State 7 keeps the match running (the modal pump's Main_Tick).
            let ran_on = self.page_tick.is_none_or(|sent| {
                state
                    .match_state
                    .sim_runtime
                    .as_ref()
                    .is_some_and(|runtime| runtime.simulation.session().tick() > sent)
            });
            return Ok(self.phase == WolPhase::Settle(0)
                && state.frontend.screen == GameScreen::InGame
                && state.match_state.match_presentation.in_game_menu == menu
                && ran_on
                && (!matches!(
                    self.target,
                    WolTarget::IngameConnectionLost
                        | WolTarget::IngamePage
                        | WolTarget::IngamePageCancel
                ) || revealed));
        }
        Ok(self.phase == WolPhase::Settle(0)
            && state.frontend.shell_exit.is_none()
            && (backdrop || state.frontend.shell_status_line.is_terminal())
            && state.frontend.shell_first_paint_slide.is_none())
    }

    pub(super) fn receipt(&self, state: &AppState) -> Value {
        let visit = state.frontend.wol.as_ref();
        json!({
            "target": format!("{:?}", self.target),
            "route": self.route,
            "service_boundary": "controlled decoded WOL results; no WOLAPI, transport or backend claim",
            "shell_route": format!("{:?}", state.frontend.shell_route),
            "mode": visit.and_then(|visit| visit.mode()).map(|mode| mode.native()),
            "authenticated": visit.is_some_and(|visit| visit.authenticated()),
            "wolinfo": state.platform.game_config.as_ref().and_then(|config| {
                std::fs::read_to_string(config.paths.settings_dir().join(WOLINFO_INI)).ok()
            }),
            "login": visit.and_then(|visit| visit.login()).map(|login| json!({
                "nickname": login.nickname.display_text(),
                "password_units": login.password.units.len(),
                "login_enabled": login.login_enabled,
                "forget_enabled": login.forget_enabled,
                "remember_enabled": login.remember_enabled,
                "remember_checked": login.remember_checked,
                "focused": login.focused_control,
            })),
        })
    }
}

fn member(name: &str, flags: u32, tag: &str) -> WolMember {
    WolMember {
        flags,
        name: name.into(),
        clan_name: if tag.is_empty() {
            String::new()
        } else {
            "Westwood Warriors".into()
        },
        clan_tag: tag.into(),
    }
}

fn lobbies() -> Vec<WolChannel> {
    [(0, 3), (1, 12), (9, 0)]
        .into_iter()
        .map(|(number, users)| WolChannel {
            kind: 0,
            max_users: 100,
            users,
            name: format!("Lob_41_{number}"),
            ping: -1,
            ..Default::default()
        })
        .collect()
}

/// An open game with a free observer slot, a full one, one of another
/// exInfo version and a keyed one limited to 800x600.
fn games() -> Vec<WolChannel> {
    let game = |name: &str, users, max_users, ex_info: &str, ping| WolChannel {
        kind: GAME_CHANNEL,
        max_users,
        users,
        ping,
        name: name.into(),
        ex_info: ex_info.into(),
        ..Default::default()
    };
    let mut keyed = game("Delta's_game", 2, 6, "25,0,0,0,0,d.map", 80);
    keyed.flags = CHANNEL_KEYED;
    keyed.game_flags = GAME_LIMIT_800;
    vec![
        game("Alpha's_game", 2, 8, "25,0,1,0,1,a.map", 45),
        game("Bravo's_game", 3, 4, "25,0,1,0,0,b.map", 120),
        game("Charlie's_game", 1, 8, "24,0,0,0,0,c.map", -1),
        keyed,
    ]
}

/// The chat sink's OnJoin for `Lob_41_0` with its users.
fn join_lobby(state: &mut AppState) {
    service_event(
        state,
        WolServiceEvent::Joined(Ok(WolJoined {
            channel: lobbies()[0].clone(),
            members: vec![
                member("Hotel", MEMBER_OPERATOR, ""),
                member(NICKNAME, 0, ""),
                member("Echo", 0, "WW"),
                member("Foxtrot", MEMBER_VOICE, ""),
            ],
        })),
    );
}

fn wol_setup_settled(state: &AppState, host: bool) -> bool {
    MoviesCapture::slide_settled(
        state,
        ShellSlideKind::NetworkSetup(crate::ui::multiplayer_setup::SetupDialog {
            host,
            flavor: crate::ui::multiplayer_setup::SetupFlavor::Wol,
        }),
    ) && state
        .frontend
        .multiplayer_setup
        .as_ref()
        .is_some_and(|ui| ui.statics.is_terminal())
}

/// A press on one `0x2BC` checkbox's 18x18 icon.
fn options_checkbox(state: &mut AppState, id: u16) -> Result<()> {
    let layout = crate::app::shell_wol::options::layout(state);
    let rect = layout.checkbox_rect(id).context("0x2BC checkbox")?;
    welcome_click(
        state,
        crate::ui::skirmish_shell::checkbox_icon_rect(rect, layout.page.scale),
    );
    Ok(())
}

fn options_button(state: &mut AppState, id: u16) -> Result<()> {
    let layout = crate::app::shell_wol::options::layout(state);
    welcome_click(state, layout.page.button_rect(id).context("0x2BC button")?);
    Ok(())
}

fn ra2md_path(state: &AppState) -> Result<std::path::PathBuf> {
    Ok(state
        .platform
        .game_config
        .as_ref()
        .context("capture game config")?
        .paths
        .settings_dir()
        .join("RA2MD.INI"))
}

fn read_ra2md(state: &AppState) -> Result<Option<Vec<u8>>> {
    let path = ra2md_path(state)?;
    match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

fn restore_ra2md(state: &AppState, bytes: Option<Vec<u8>>) -> Result<()> {
    let path = ra2md_path(state)?;
    match bytes {
        Some(bytes) => std::fs::write(&path, bytes),
        None => std::fs::remove_file(&path),
    }
    .with_context(|| format!("restore {}", path.display()))
}

fn lobby_button(state: &mut AppState, layout: &lobby_ui::WolLobbyLayout, id: u16) -> Result<()> {
    welcome_click(state, layout.page.button_rect(id).context("0x113 button")?);
    Ok(())
}

/// A click on one visible game row.
fn game_row_click(state: &mut AppState, row: usize) -> Result<()> {
    let layout = crate::app::shell_wol::lobby::layout(state);
    let lobby = state
        .frontend
        .wol
        .as_ref()
        .and_then(|visit| visit.lobby())
        .context("the lobby runner")?;
    let geometry =
        lobby
            .ui
            .game_list
            .geometry(layout.games, lobby.ui.game_rows.len(), layout.page.scale);
    let rect = geometry.row(row - lobby.ui.game_list.top_index);
    welcome_click(state, rect);
    Ok(())
}

/// Three IRC records; the preferred-server draw left record 1 selected.
/// `wolinfoMD.ini` beside the isolated RA2MD.INI.
fn info_file(state: &AppState) -> Result<WolInfoFile> {
    let config = state
        .platform
        .game_config
        .as_ref()
        .context("capture game config")?;
    Ok(WolInfoFile::in_directory(config.paths.settings_dir()))
}

/// The target's persona file, only in the marked isolated root.
fn write_wolinfo(state: &AppState, target: WolTarget) -> Result<()> {
    let root = super::lan::LanCapture::isolated_root(state)?;
    let config = state
        .platform
        .game_config
        .as_ref()
        .context("capture game config")?;
    ensure!(
        std::fs::canonicalize(config.paths.settings_dir())? == root,
        "wolinfoMD.ini belongs to the isolated root only"
    );
    std::fs::write(root.join(WOLINFO_INI), target.wolinfo()).context("write wolinfoMD.ini")
}

fn server_list() -> WolServerList {
    WolServerList {
        servers: ["Pacific", "Westwood", "Europe"]
            .into_iter()
            .map(|name| WolServer {
                name: format!("irc.westwood.com:{name}"),
            })
            .collect(),
        selected: 1,
    }
}

fn button(layout: &crate::ui::wol_login::WolLoginLayout, id: u16) -> Result<RectPx> {
    layout.page.button_rect(id).context("0x10F button")
}

/// A press and release through the window handlers' WOL routing; like the
/// native helper, the pointer then returns to the neutral point.
fn welcome_click(state: &mut AppState, rect: RectPx) {
    set_window_pointer_to_rect_center(state, rect);
    App::handle_wol_mouse_move(state);
    App::handle_wol_mouse_down(state);
    App::handle_wol_mouse_up(state);
    MoviesCapture::restore_neutral_pointer(state);
}

fn login_click(state: &mut AppState, rect: RectPx) {
    // The combo face's left part is its NewEdit, clear of the arrow.
    let rect = RectPx::new(rect.x, rect.y, rect.w / 2, rect.h);
    welcome_click(state, rect);
}

fn setup_view(state: &AppState) -> Result<crate::ui::multiplayer_setup::SetupView> {
    setup::view(state).context("C2/C9 view")
}

fn setup_requests(state: &mut AppState) -> Vec<NetworkSetupRequest> {
    state
        .frontend
        .network_setup_protocol
        .as_mut()
        .map(|protocol| protocol.take_service_requests())
        .unwrap_or_default()
}

fn setup_chat_has(state: &AppState, text: &str) -> Result<bool> {
    Ok(state
        .frontend
        .multiplayer_setup
        .as_ref()
        .context("C2/C9")?
        .chat
        .lines
        .iter()
        .any(|line| String::from_utf16_lossy(&line.text) == text))
}

fn setup_click(state: &mut AppState, rect: RectPx) {
    set_window_pointer_to_rect_center(state, rect);
    crate::app::input::multiplayer_setup::cursor_moved(state);
    crate::app::input::multiplayer_setup::mouse(state, winit::event::MouseButton::Left, true);
    crate::app::input::multiplayer_setup::mouse(state, winit::event::MouseButton::Left, false);
}

fn setup_button(state: &mut AppState, id: u16) -> Result<()> {
    let rect = crate::app::input::multiplayer_setup::layout(state)
        .page
        .buttons
        .into_iter()
        .find(|button| button.id == id)
        .context("setup button")?
        .rect;
    setup_click(state, rect);
    Ok(())
}

/// My Information `0x114`: the driver's requests, the page's init and one
/// control per target, each checked against the model and `wolinfoMD.ini`.
impl WolCapture {
    fn my_info_open(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::ui::wol_shell::WolMode;
        let requests = self.requests(state, frame)?;
        ensure!(
            requests == [WolServiceRequest::EnsureApi],
            "0x799D30 did not start with the WOLAPI object: {requests:?}"
        );
        ensure!(
            state.frontend.wol.as_ref().and_then(|visit| visit.mode())
                == Some(WolMode::MyInformation),
            "My Information did not write mode 6"
        );
        service_event(state, WolServiceEvent::Api(WolApiStatus::Available));
        let requests = self.requests(state, frame)?;
        ensure!(
            requests
                == [
                    WolServiceRequest::ListCredentials,
                    WolServiceRequest::FetchServers { flags: 0x2900 }
                ],
            "0x799D30 did not wait for the server list: {requests:?}"
        );
        service_event(
            state,
            WolServiceEvent::Credentials(vec![
                WolCredential {
                    slot: 1,
                    nickname: NICKNAME.into(),
                    password: PASSWORD.into(),
                },
                WolCredential {
                    slot: 2,
                    nickname: PEER.into(),
                    password: PASSWORD.into(),
                },
            ]),
        );
        service_event(state, WolServiceEvent::NetStatus(0x0004_012F));
        service_event(state, WolServiceEvent::ServerList(Ok(server_list())));
        ensure!(
            crate::app::shell_wol::my_info::page(state).is_some(),
            "the server list did not open 0x114"
        );
        let requests = self.requests(state, frame)?;
        ensure!(
            requests == [WolServiceRequest::LocaleCount],
            "0x114's locale combo did not ask for the count: {requests:?}"
        );
        service_event(state, WolServiceEvent::LocaleCount(38));
        ensure!(
            !state
                .frontend
                .wol
                .as_ref()
                .is_some_and(|visit| visit.authenticated()),
            "My Information must not log in"
        );
        self.event(
            frame,
            "controlled WOLAPI, 2 stored accounts, 3 server records",
        );
        self.phase = WolPhase::MyInfoShown;
        Ok(())
    }

    fn my_info_act(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::ui::wol_my_info::{
            AUTO_LOGIN, BACK, COUNTRY, FORGET, HEADING, LOCALE, NICKNAME as NICKNAME_COMBO, SERVER,
            WINS,
        };
        let page = crate::app::shell_wol::my_info::page(state).context("0x114")?;
        let data: Vec<i32> = page.combos.iter().map(|combo| combo.data_or(-99)).collect();
        let snapshot = json!({
            "selected_data": data,
            "server_face": page.combos[1].selected.map(|row| page.combos[1].rows[row].text.clone()),
            "nicknames": page.combos[0].rows.iter().map(|row| row.text.clone()).collect::<Vec<_>>(),
            "auto_login": [page.auto_login, page.auto_login_enabled],
            "match_same_res": page.match_same_res,
            "forget_enabled": page.forget_enabled,
            "play_buddy_enabled": page.play_buddy_enabled,
            "texts": page.texts,
        });
        if self.target == WolTarget::MyInfoDefault {
            ensure!(
                page.slot() == -1
                    && !page.forget_enabled
                    && !page.play_buddy_enabled
                    && !page.auto_login_enabled
                    // WOL entry allocated the empty nickname, which equals
                    // the empty AutoLogin: checked, though disabled.
                    && page.auto_login
                    && page
                        .texts
                        .iter()
                        .any(|(id, text)| *id == HEADING && text.is_empty()),
                "0x114 without a nickname: {snapshot}"
            );
        } else {
            // Westwood is record 1 (the first row); locale 2, country 3 and
            // colour 2 by item data.
            ensure!(
                data == [1, 1, 2, 3, 2]
                    && page.forget_enabled
                    && page.play_buddy_enabled
                    && page.auto_login_enabled
                    && !page.auto_login
                    && !page.match_same_res
                    && page
                        .texts
                        .iter()
                        .any(|(id, text)| *id == WINS && text == "21"),
                "0x114 did not fill from LastValid's persona: {snapshot}"
            );
        }
        self.event(frame, json!({"0x114 init": snapshot}));
        let file = info_file(state)?;
        let persona = |_: &AppState| file.load_persona(NICKNAME, &|key| key.to_owned());
        match self.target {
            WolTarget::MyInfoServer => {
                let row = self.my_info_pick(state, SERVER, |row| row.text == "Europe")?;
                let saved = persona(state).server;
                ensure!(saved == "Europe", "the server was not saved: {saved}");
                self.event(frame, json!({"0x629 row": row, "Server": saved}));
            }
            WolTarget::MyInfoCountry => {
                let row = self.my_info_pick(state, COUNTRY, |row| row.data == 5)?;
                let saved = persona(state).country;
                ensure!(saved == 5, "the country was not saved: {saved}");
                self.event(frame, json!({"0x6A1 row": row, "Country": saved}));
            }
            WolTarget::MyInfoLocale => {
                let row = self.my_info_pick(state, LOCALE, |row| row.data == 5)?;
                let saved = persona(state);
                ensure!(
                    saved.locale == 5 && !saved.locale_verified,
                    "the locale was not saved unverified: {saved:?}"
                );
                self.event(
                    frame,
                    json!({"0x62A row": row, "Locale": 5, "LocaleVerified": false}),
                );
            }
            WolTarget::MyInfoAutoLogin => {
                let mut seen = Vec::new();
                for _ in 0..3 {
                    self.my_info_check(state, AUTO_LOGIN)?;
                    seen.push(file.auto_login());
                }
                let seen: Vec<&str> = seen.iter().map(String::as_str).collect();
                ensure!(
                    seen == [NICKNAME, "", NICKNAME],
                    "AutoLogin did not follow the box: {seen:?}"
                );
                self.event(frame, json!({"0x6FF AutoLogin": seen}));
            }
            WolTarget::MyInfoForget => {
                let rect = layout_button(state, FORGET)?;
                welcome_click(state, rect);
                let requests = self.requests(state, frame)?;
                let page = crate::app::shell_wol::my_info::page(state).context("0x114")?;
                let names: Vec<&str> = page.combos[0]
                    .rows
                    .iter()
                    .map(|row| row.text.as_str())
                    .collect();
                ensure!(
                    requests == [WolServiceRequest::ForgetCredential { slot: 1 }]
                        && names.len() == 2
                        && names[1] == PEER
                        && page.slot() == -1
                        && !page.forget_enabled
                        && page.play_buddy_enabled
                        && !page.auto_login_enabled
                        && file.last_valid().is_empty()
                        && state.frontend.wol_account.nickname == NICKNAME,
                    "Forget did not follow 0x799692: {names:?} {requests:?}"
                );
                self.event(frame, json!({"0x6EB Forget": names, "LastValid": ""}));
            }
            WolTarget::MyInfoBack => {
                self.my_info_pick(state, NICKNAME_COMBO, |row| row.text == PEER)?;
                ensure!(
                    state.frontend.wol_account.nickname == PEER,
                    "the nickname combo did not set [0xB73E40]"
                );
                let rect = layout_button(state, BACK)?;
                welcome_click(state, rect);
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "Back did not tear 0x114 down"
                );
                self.event(frame, "0x628 Alpha, then 0x114 Back");
                self.phase = WolPhase::MyInfoBack;
                return Ok(());
            }
            _ => {}
        }
        self.hover_settle(state);
        Ok(())
    }

    /// Open `control`'s drop list by its arrow and click the first row that
    /// `matches`; returns that row.
    fn my_info_pick(
        &mut self,
        state: &mut AppState,
        control: u16,
        matches: impl Fn(&crate::ui::wol_my_info::MyInfoRow) -> bool,
    ) -> Result<usize> {
        let layout = crate::app::shell_wol::my_info::layout(state);
        let face = layout.face(control).context("combo face")?;
        let arrow = crate::ui::skirmish_shell::combo_arrow_hit_rect(face, layout.page.scale);
        welcome_click(state, arrow);
        let page = crate::app::shell_wol::my_info::page(state).context("0x114")?;
        ensure!(
            page.popup.control == Some(control),
            "the drop list did not open"
        );
        let row = page
            .combo(control)
            .and_then(|combo| combo.rows.iter().position(&matches))
            .context("no matching row")?;
        let visible = page
            .popup_geometry(&layout)
            .context("drop list")?
            .visible_rows;
        // A row below the first page: scroll there first, as the bar would.
        if row >= visible
            && let crate::app::shell_wol::WolPage::MyInformation(page) =
                &mut state.frontend.wol.as_mut().context("WOL visit")?.page
        {
            page.popup.top_index = row + 1 - visible;
        }
        let popup = crate::app::shell_wol::my_info::page(state)
            .and_then(|page| page.popup_geometry(&layout))
            .context("drop list")?;
        let rect = popup.row_rect(row).context("row rect")?;
        welcome_click(state, rect);
        Ok(row)
    }

    /// A click on a checkbox's icon.
    fn my_info_check(&mut self, state: &mut AppState, control: u16) -> Result<()> {
        let layout = crate::app::shell_wol::my_info::layout(state);
        let rect = if control == crate::ui::wol_my_info::AUTO_LOGIN {
            layout.auto_login
        } else {
            layout.match_same_res
        };
        welcome_click(
            state,
            crate::ui::skirmish_shell::checkbox_icon_rect(rect, layout.page.scale),
        );
        Ok(())
    }
}

fn layout_button(state: &AppState, id: u16) -> Result<RectPx> {
    crate::app::shell_wol::my_info::layout(state)
        .page
        .button_rect(id)
        .context("0x114 button")
}

/// The Buddy List `0x116` on the test service: the located rows, then one
/// action per target through the production input routing.
impl WolCapture {
    /// The stand-ins' buddies and the statuses their locates give: Tanya in
    /// chat lobby 0, Boris in his game, Ghost offline, Ivan's find off.
    const BUDDIES: [(&str, i32); 4] = [("Tanya", 4), ("Boris", 5), ("Ghost", 2), ("Ivan", 1)];

    fn buddy_hosts(&self) -> bool {
        matches!(
            self.target,
            WolTarget::BuddyCreate | WolTarget::BuddyCreateBack
        )
    }

    fn buddy_my_info(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::ui::wol_my_info::PLAY_BUDDY;
        let page = crate::app::shell_wol::my_info::page(state).context("0x114")?;
        if self.rejoined {
            ensure!(
                state.frontend.wol.as_ref().and_then(|visit| visit.mode())
                    == Some(crate::ui::wol_shell::WolMode::MyInformation),
                "0x116 Back did not return to mode 6"
            );
            self.event(frame, "0x114 settled again after 0x116 Back");
            self.hover_settle(state);
            return Ok(());
        }
        ensure!(page.play_buddy_enabled, "0x114 Play Buddy stayed disabled");
        welcome_click(state, layout_button(state, PLAY_BUDDY)?);
        ensure!(
            state.frontend.shell_exit.is_some(),
            "Play Buddy did not tear 0x114 down"
        );
        self.event(frame, "0x114 Play Buddy");
        self.phase = WolPhase::ServiceLogin;
        Ok(())
    }

    /// `0x116` and its mirror: the rows once every locate has answered.
    fn buddy_rows(state: &AppState) -> Result<Vec<(String, Option<i32>, String)>> {
        let dialog = crate::app::shell_wol::buddy::page(state).context("0x116")?;
        Ok(dialog
            .ui
            .rows
            .iter()
            .map(|row| {
                (
                    row.name.clone(),
                    state.frontend.wol_buddies.status(&row.name),
                    row.status.clone(),
                )
            })
            .collect())
    }

    fn buddy_row_click(state: &mut AppState, name: &str) -> Result<()> {
        let layout = crate::app::shell_wol::buddy::layout(state);
        let dialog = crate::app::shell_wol::buddy::page(state).context("0x116")?;
        let ui = &dialog.ui;
        let row = ui
            .rows
            .iter()
            .position(|row| row.name == name)
            .context("no buddy row")?;
        let rect = ui
            .list
            .geometry(layout.list, ui.rows.len(), layout.page.scale)
            .row(row - ui.list.top_index);
        welcome_click(state, rect);
        ensure!(
            state.frontend.wol_buddies.selected == name,
            "the row click did not select {name}"
        );
        Ok(())
    }

    fn buddy_button(state: &mut AppState, id: u16) -> Result<()> {
        let rect = crate::app::shell_wol::buddy::layout(state)
            .page
            .button_rect(id)
            .context("0x116 button")?;
        welcome_click(state, rect);
        Ok(())
    }

    fn buddy_shown(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::ui::wol_buddy::{ADD, BACK, CREATE, JOIN, PAGE_BUTTON, REMOVE, WINS};
        let rows = Self::buddy_rows(state)?;
        let expected: Vec<(&str, Option<i32>)> = Self::BUDDIES
            .iter()
            .map(|(name, status)| (*name, Some(*status)))
            .collect();
        let got: Vec<(&str, Option<i32>)> = rows
            .iter()
            .map(|(name, status, _)| (name.as_str(), *status))
            .collect();
        if got != expected {
            // The list and the locates answer over the next frames.
            return Ok(());
        }
        self.event(frame, json!({"0x116 rows": rows}));
        if self.rejoined {
            self.event(frame, "0x116 settled again");
            self.hover_settle(state);
            return Ok(());
        }
        match self.target {
            WolTarget::BuddySteady => {
                Self::buddy_row_click(state, "Tanya")?;
                let dialog = crate::app::shell_wol::buddy::page(state).context("0x116")?;
                let texts = dialog.ui.texts.clone().unwrap_or_default();
                ensure!(
                    texts.iter().any(|(id, text)| *id == WINS && text == "12")
                        && dialog.ui.enabled(JOIN)
                        && dialog.ui.enabled(PAGE_BUTTON)
                        && dialog.ui.enabled(REMOVE),
                    "Tanya's selection did not show her ladder record: {texts:?}"
                );
                self.event(frame, json!({"Tanya selected": texts}));
            }
            WolTarget::BuddyAddPopup | WolTarget::BuddyAdded => {
                Self::buddy_button(state, ADD)?;
                App::handle_wol_key(state, None, true, Some("Natasha"));
                let dialog = crate::app::shell_wol::buddy::page(state).context("0x116")?;
                let add = dialog.ui.add.as_ref().context("Add did not open 0x115")?;
                ensure!(
                    add.text() == "Natasha" && add.add_enabled,
                    "0x115 did not take the name: {:?}",
                    add.text()
                );
                self.event(frame, "0x115 opened, Natasha typed");
                if self.target == WolTarget::BuddyAdded {
                    App::handle_wol_key(state, Some(winit::keyboard::KeyCode::Enter), true, None);
                    let rows = Self::buddy_rows(state)?;
                    ensure!(
                        crate::app::shell_wol::buddy::page(state)
                            .is_some_and(|dialog| dialog.ui.add.is_none())
                            && rows.iter().any(|(name, _, _)| name == "Natasha"),
                        "Enter did not add Natasha: {rows:?}"
                    );
                    self.event(frame, json!({"0x115 Enter": rows}));
                    self.phase = WolPhase::BuddyStored;
                    return Ok(());
                }
            }
            WolTarget::BuddyRemove => {
                Self::buddy_row_click(state, "Ghost")?;
                Self::buddy_button(state, REMOVE)?;
                let rows = Self::buddy_rows(state)?;
                ensure!(
                    rows.len() == 3
                        && !rows.iter().any(|(name, _, _)| name == "Ghost")
                        && state.frontend.wol_buddies.selected.is_empty(),
                    "Remove did not delete Ghost: {rows:?}"
                );
                self.event(frame, json!({"Ghost removed": rows}));
                self.phase = WolPhase::BuddyStored;
                return Ok(());
            }
            WolTarget::BuddyPage => {
                Self::buddy_row_click(state, "Tanya")?;
                Self::buddy_button(state, PAGE_BUTTON)?;
                let received = crate::app::shell_wol::page::page_received(state)
                    .context("Page did not open 0x11E")?;
                ensure!(
                    received.reply_to == "Tanya" && received.lines.is_empty(),
                    "0x11E did not reply to Tanya: {:?}",
                    received.reply_to
                );
                self.event(frame, "Page: modal 0x11E to Tanya");
            }
            WolTarget::BuddyJoinGame | WolTarget::BuddyJoinGameBack => {
                Self::buddy_row_click(state, "Boris")?;
                Self::buddy_button(state, JOIN)?;
                self.event(frame, "Join on Boris (in Boris's_game)");
                self.phase = WolPhase::BuddySetup;
                return Ok(());
            }
            WolTarget::BuddyJoinLobby => {
                Self::buddy_row_click(state, "Tanya")?;
                Self::buddy_button(state, JOIN)?;
                self.event(frame, "Join on Tanya (in a chat lobby)");
                self.phase = WolPhase::BuddyReturn;
                return Ok(());
            }
            WolTarget::BuddyCreate | WolTarget::BuddyCreateBack => {
                Self::buddy_button(state, CREATE)?;
                self.event(frame, "0x116 Create Game");
                self.phase = WolPhase::BuddyCreatePopup;
                return Ok(());
            }
            WolTarget::BuddyBack | WolTarget::BuddyFromLobby | WolTarget::BuddyFromMyInfo => {
                Self::buddy_button(state, BACK)?;
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "Back did not tear 0x116 down"
                );
                self.event(frame, "0x116 Back");
                self.rejoined = true;
                self.phase = if self.target == WolTarget::BuddyFromMyInfo {
                    WolPhase::BuddyMyInfo
                } else {
                    WolPhase::BuddyReturn
                };
                return Ok(());
            }
            _ => {}
        }
        self.hover_settle(state);
        Ok(())
    }

    fn buddy_setup(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        ensure!(
            state
                .match_state
                .session
                .network_setup()
                .is_some_and(|launch| launch.local_player().name == NICKNAME),
            "C2/C9 did not open for the local user"
        );
        self.event(frame, "C2/C9 settled from 0x116");
        if matches!(
            self.target,
            WolTarget::BuddyJoinGameBack | WolTarget::BuddyCreateBack
        ) {
            setup_button(state, crate::ui::multiplayer_setup::BACK)?;
            self.event(frame, "C2/C9 Back");
            self.rejoined = true;
            self.phase = WolPhase::BuddyShown;
            return Ok(());
        }
        MoviesCapture::restore_neutral_pointer(state);
        crate::app::input::multiplayer_setup::cursor_moved(state);
        self.settle();
        Ok(())
    }

    fn buddy_returned(&self, state: &AppState, rendered: PresentedShell) -> bool {
        let (shell, kind) = match self.target {
            WolTarget::BuddyBack => (PresentedShell::WolWelcome, ShellSlideKind::WolWelcome),
            _ => (PresentedShell::WolLobby, ShellSlideKind::WolLobby),
        };
        rendered == shell && Self::page_settled(state, kind)
    }

    fn buddy_return(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let visit = state.frontend.wol.as_ref().context("WOL visit")?;
        if self.target == WolTarget::BuddyBack {
            ensure!(
                visit.mode().is_none() && visit.return_mode().is_none(),
                "0x116 Back did not return to mode 0"
            );
            self.event(frame, "0x10E settled after 0x116 Back");
        } else {
            let lobby = Self::lobby(state)?;
            ensure!(
                lobby.shown && lobby.runner == crate::app::shell_wol::lobby::Runner::Lobby,
                "0x113 did not run the lobby runner"
            );
            self.event(
                frame,
                json!({"0x113 settled after 0x116": state.frontend.wol_lobby_globals.lobby_number}),
            );
        }
        self.hover_settle(state);
        Ok(())
    }
}

/// C9 in the test service's game: the host's options, then the target's
/// departure or kick through the production lobby sinks.
impl WolCapture {
    /// Test-service C2 with its stand-in guest: the join sink `0x7A6100`,
    /// the first `R` (`0x7A8260`) and `A1` (`0x77E900`).
    fn service_host(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::skirmish_launch::{LaunchCountry, LaunchStartPosition, LaunchTeam};
        let guest = |state: &AppState| {
            let launch = state.match_state.session.network_setup()?;
            let index = launch
                .humans
                .iter()
                .position(|human| human.name == SERVICE_GUEST)?;
            Some((index, launch.humans[index].clone()))
        };
        let (index, human) = guest(state).context("the stand-in guest did not join")?;
        let view = setup_view(state)?;
        let accepted = human
            .raw_slot_index
            .and_then(|row| view.row_icons[usize::from(row)].as_ref())
            .is_some_and(|icon| icon.accepted);
        ensure!(
            human.raw_slot_index == Some(1)
                && human.slot.country == LaunchCountry::Korea
                && !human.slot.country_random
                && human.slot.color_index == 3
                && !human.slot.color_random
                && state.match_state.session.network_player_ready(index)
                && state.match_state.session.network_map_status(index) == Some(1)
                && accepted,
            "the stand-in guest's row, choices, K1 or acceptance: {human:?}, icon {accepted}"
        );
        self.event(
            frame,
            json!({"guest joined": {"row": 1, "country": "Korea", "color": 3, "K": 1, "accepted": true}}),
        );
        let choices = |state: &mut AppState, slot, observer| {
            crate::app::shell_wol::service_event(
                state,
                WolServiceEvent::PlayerChoices {
                    from: SERVICE_GUEST.into(),
                    slot,
                    observer,
                },
            );
            setup_requests(state)
        };
        match self.target {
            WolTarget::GuestChoices => {
                // The host's colour 5 first, then the guest asks for it.
                crate::app::shell_network_setup::apply_action(
                    state,
                    crate::ui::multiplayer_setup::SetupAction::Select {
                        control: crate::ui::multiplayer_setup::SetupColumn::Color.control(0),
                        value: 5,
                    },
                );
                let host = state
                    .match_state
                    .session
                    .network_setup()
                    .context("C2")?
                    .local_player()
                    .slot
                    .clone();
                ensure!(
                    host.color_index == 5 && !host.color_random,
                    "the host's colour 5: {host:?}"
                );
                let requests = choices(
                    state,
                    crate::skirmish_launch::SkirmishLocalSlot {
                        color_index: 5,
                        start_position: LaunchStartPosition::Position(0),
                        team: LaunchTeam::Team(7),
                        ..human.slot.clone()
                    },
                    false,
                );
                let (_, human) = guest(state).context("guest")?;
                ensure!(
                    human.slot.color_random
                        && human.slot.start_position == LaunchStartPosition::Position(0)
                        && human.slot.team == LaunchTeam::None
                        && requests.contains(&NetworkSetupRequest::PublishOptions),
                    "the R checks: {human:?} {requests:?}"
                );
                self.event(
                    frame,
                    json!({"guest R": "colour 5 taken -> Random, start 1, team 7 -> none"}),
                );
            }
            WolTarget::GuestObserver => {
                // The host opens the observer row 7 first (player combo 7).
                crate::app::shell_network_setup::apply_action(
                    state,
                    crate::ui::multiplayer_setup::SetupAction::Select {
                        control: crate::ui::multiplayer_setup::SetupColumn::Player.control(7),
                        value: 7,
                    },
                );
                let open_observer = state.match_state.session.network_free_row(true);
                ensure!(
                    open_observer == Some(7),
                    "C2 did not open observer row 7: {:?}",
                    setup_view(state)?
                        .combo(crate::ui::multiplayer_setup::SetupColumn::Player.control(7))
                );
                setup_requests(state);
                let requests = choices(state, human.slot.clone(), true);
                let (_, observer) = guest(state).context("guest")?;
                let roster = [
                    NetworkSetupRequest::Advertise,
                    NetworkSetupRequest::PublishRoster,
                    NetworkSetupRequest::PublishOptions,
                ];
                ensure!(
                    requests == roster
                        && match open_observer {
                            Some(_) => {
                                observer.is_observer()
                                    && observer.raw_slot_index
                                        == state.match_state.session.network_observer_row()
                            }
                            None => !observer.is_observer() && observer.raw_slot_index.is_none(),
                        },
                    "the observer R with observer row {open_observer:?}: {observer:?} {requests:?}"
                );
                self.event(frame, json!({"guest observes": {"open observer row": open_observer, "row": observer.raw_slot_index}}));
                if open_observer.is_some() {
                    let requests = choices(state, human.slot.clone(), false);
                    let (_, player) = guest(state).context("guest")?;
                    // 0x7A8838: country Random, then the R's Korea differs.
                    ensure!(
                        !player.is_observer()
                            && player.raw_slot_index == Some(1)
                            && player.slot.country == LaunchCountry::Korea
                            && !player.slot.country_random
                            && requests == roster,
                        "the player R: {player:?} {requests:?}"
                    );
                    self.event(frame, json!({"guest plays again": {"row": 1}}));
                }
            }
            WolTarget::ScenariosMismatch | WolTarget::MismatchOk => {
                setup_button(state, crate::ui::multiplayer_setup::START)?;
                self.event(frame, "C2 Start with the stand-in guest");
                self.phase = WolPhase::Mismatch;
                return Ok(());
            }
            WolTarget::GuestKick => {
                crate::app::shell_network_setup::apply_action(
                    state,
                    crate::ui::multiplayer_setup::SetupAction::Select {
                        control: crate::ui::multiplayer_setup::SetupColumn::Player.control(1),
                        value: 2,
                    },
                );
                self.event(frame, "C2 kicks the stand-in guest");
                self.phase = WolPhase::GuestKicked;
                return Ok(());
            }
            _ => {}
        }
        MoviesCapture::restore_neutral_pointer(state);
        crate::app::input::multiplayer_setup::cursor_moved(state);
        self.settle();
        Ok(())
    }

    /// The host's map changes to a version this guest cannot verify (its
    /// digest differs), so the guest has no preview and asks for one.
    fn guest_preview(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let preview = |state: &AppState| {
            let protocol = state
                .frontend
                .network_setup_protocol
                .as_ref()
                .expect("C9 protocol");
            (
                protocol.local_preview(),
                protocol.received_preview().is_some(),
                protocol.wol_no_preview(),
            )
        };
        ensure!(
            preview(state) == (true, false, (false, false)),
            "the host's first map did not preview locally: {:?}",
            preview(state)
        );
        let launch = state
            .match_state
            .session
            .network_setup()
            .context("C9 setup")?
            .clone();
        let file = launch.selected_map_file.clone().context("host map")?;
        let mut map_metadata = crate::app::shell_network_setup::local_map_metadata(state, &file)?;
        map_metadata.digest = "another version".into();
        let seed = state.match_state.session.network_start_seed();
        let publish = |state: &mut AppState| {
            setup::service_event(
                state,
                NetworkSetupEvent::HostSetup {
                    launch: launch.clone(),
                    map_metadata: map_metadata.clone(),
                    vacancies: [crate::app::match_runtime::session::NetworkVacancy::Open; 8],
                    observer_row: None,
                    cooperative_difficulty: 0,
                    seed,
                },
            )?;
            let requests = setup_requests(state);
            Ok::<_, anyhow::Error>(
                requests
                    .iter()
                    .filter(|request| **request == NetworkSetupRequest::RequestPreview)
                    .count(),
            )
        };
        setup_requests(state);
        ensure!(
            publish(state)? == 1 && preview(state) == (false, false, (false, false)),
            "the unverified map did not send one P: {:?}",
            preview(state)
        );
        ensure!(publish(state)? == 0, "an unchanged options text sent P");
        self.event(frame, json!({"P": file, "digest": "another version"}));
        if self.target == WolTarget::GuestNoPreview {
            // 0x7A8059..0x7A80D7: 900 buckets on, each options text asks
            // again without restamping.
            let stamp = state
                .frontend
                .wol_lobby_globals
                .preview_stamp
                .context("the sink's stamp")?;
            state.frontend.wol_lobby_globals.preview_stamp = Some(stamp.wrapping_sub(901));
            ensure!(
                publish(state)? == 1 && publish(state)? == 1,
                "the 900-bucket rule did not ask again on each options text"
            );
            self.event(frame, "P again on each options text after 900 buckets");
            setup::service_event(state, NetworkSetupEvent::PreviewUnavailable)?;
            let view = setup_view(state)?;
            ensure!(
                view.no_preview && view.preview_hidden,
                "Z3 did not show 0x71F over a hidden 0x468"
            );
            self.event(frame, "Z3: 0x71F Preview unavailable, 0x468 hidden");
            return Ok(());
        }
        setup::service_event(state, NetworkSetupEvent::PreviewOffered)?;
        ensure!(
            setup_requests(state)
                == [
                    NetworkSetupRequest::AcknowledgePreview,
                    NetworkSetupRequest::RequestPreviewFile
                ],
            "the WOL offer did not ack and request the file"
        );
        let (width, height) = (96u32, 96u32);
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                rgba.extend_from_slice(&[
                    (40 + x) as u8,
                    (60 + y) as u8,
                    ((x + y) / 2) as u8,
                    0xFF,
                ]);
            }
        }
        let preview_bin =
            crate::map::preview::preview_bin_bytes(&crate::map::preview::create_paletted_preview(
                &crate::map::preview::DecodedPreview {
                    width,
                    height,
                    rgba,
                },
                crate::map::preview::PALETTED_PREVIEW_COLORS,
            ));
        let root = super::lan::LanCapture::isolated_root(state)?;
        std::fs::write(root.join("Preview.bin"), preview_bin)?;
        setup::service_event(
            state,
            NetworkSetupEvent::PreviewFileReceived {
                transferred_file: Some("Preview.bin".into()),
            },
        )?;
        std::fs::remove_file(root.join("Preview.bin"))?;
        ensure!(
            preview(state) == (false, true, (false, false)) && publish(state)? == 0,
            "the received preview: {:?}",
            preview(state)
        );
        self.event(frame, "offer acked, Preview.bin taken, no further P");
        // Another guest's Z3 hides even this preview; the next public text,
        // here the host's A1, restores it (0x7A8227..0x7A824E).
        setup::service_event(state, NetworkSetupEvent::PreviewUnavailable)?;
        ensure!(
            preview(state).2 == (true, true),
            "Z3 did not hide the received preview"
        );
        setup::player_ready(state, "Tanya", 1)?;
        ensure!(
            preview(state).2 == (false, false),
            "the host's A1 did not restore 0x468"
        );
        self.event(frame, "Z3 hid the preview; the host's A1 restored it");
        Ok(())
    }

    fn guest_options(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let launch = state
            .match_state
            .session
            .network_setup()
            .context("C9 setup")?;
        let rows: Vec<(String, Option<u8>)> = launch
            .humans
            .iter()
            .map(|human| (human.name.clone(), human.raw_slot_index))
            .collect();
        let map = launch.selected_map_file.clone();
        let view = setup_view(state)?;
        let host_icon = view.row_icons[0].as_ref().is_some_and(|icon| icon.host)
            && !view.row_icons[1].as_ref().is_some_and(|icon| icon.host);
        ensure!(
            map.is_some()
                && rows
                    == [
                        (NICKNAME.to_owned(), Some(1)),
                        ("Tanya".to_owned(), Some(0))
                    ]
                && host_icon,
            "C9 did not take the host's options: {rows:?} {map:?} host icon {host_icon}"
        );
        self.event(frame, json!({"host options": {"map": map, "rows": rows}}));
        let channel = WolChannel {
            kind: GAME_CHANNEL,
            name: "Tanya's_game".into(),
            ..Default::default()
        };
        match self.target {
            WolTarget::GuestHostLeft => {
                crate::app::shell_wol::service_event(
                    state,
                    WolServiceEvent::UserLeft("Tanya".into()),
                );
                self.event(frame, "the host left the game channel");
                self.rejoined = true;
                self.phase = WolPhase::ServiceLobby;
                return Ok(());
            }
            WolTarget::GuestKicked => {
                crate::app::shell_wol::service_event(
                    state,
                    WolServiceEvent::Kicked {
                        channel,
                        user: member(NICKNAME, crate::ui::wol_lobby::MEMBER_LOCAL, ""),
                        by: member("Tanya", MEMBER_OPERATOR, ""),
                    },
                );
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "the kick did not close C9"
                );
                self.event(frame, "this guest kicked by the host");
                self.rejoined = true;
                self.phase = WolPhase::ServiceLobby;
                return Ok(());
            }
            WolTarget::GuestUserKicked => {
                crate::app::shell_wol::service_event(
                    state,
                    WolServiceEvent::UserJoined(member("Boris", 0, "")),
                );
                let joined = state
                    .match_state
                    .session
                    .network_setup()
                    .is_some_and(|launch| launch.humans.iter().any(|human| human.name == "Boris"));
                ensure!(joined, "Boris's join added no row");
                crate::app::shell_wol::service_event(
                    state,
                    WolServiceEvent::Kicked {
                        channel: channel.clone(),
                        user: member("Boris", 0, ""),
                        by: member("Tanya", MEMBER_OPERATOR, ""),
                    },
                );
                let kicked = App::csf_label(state, "TXT_USER_KICKED", "TXT_USER_KICKED");
                let expected = crate::assets::csf_file::format_csf(
                    &kicked,
                    &[
                        crate::assets::csf_file::CsfArg::Str("Boris"),
                        crate::assets::csf_file::CsfArg::Str("Tanya's_game"),
                        crate::assets::csf_file::CsfArg::Str("Tanya"),
                    ],
                );
                let gone = state
                    .match_state
                    .session
                    .network_setup()
                    .is_some_and(|launch| !launch.humans.iter().any(|human| human.name == "Boris"));
                ensure!(
                    gone && setup_chat_has(state, &expected)?,
                    "Boris's kick: row gone {gone}, line {expected:?}"
                );
                self.event(frame, "Boris joined, then was kicked");
            }
            WolTarget::GuestNoPreview | WolTarget::GuestPreviewReceived => {
                self.guest_preview(state, frame)?;
            }
            _ => {}
        }
        MoviesCapture::restore_neutral_pointer(state);
        crate::app::input::multiplayer_setup::cursor_moved(state);
        self.settle();
        Ok(())
    }
}
