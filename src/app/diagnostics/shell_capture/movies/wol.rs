//! Production WOL Custom Match checkpoints: Main Menu -> Internet -> Custom
//! Match through the real `0x10E`/`0x10F`/`0xC4`/box handlers (a modern
//! visit opens at `0x10F`), with a controlled WOL service that answers the
//! visit's own requests. Nothing here builds a page; the service events are
//! decoded results, not a transport or backend. The accepted login WRITES the MultiPlayer handle:
//! use an isolated RA2_DIR.

use super::*;
use crate::app::match_runtime::session::SessionMode;
use crate::app::persistence::wol_profile::{WOLINFO_INI, WolInfoFile};
use crate::app::shell_network_setup::{
    self as setup, NetworkSetupEvent, NetworkSetupRequest, WolStartUser,
};
use crate::app::shell_route::ShellRoute;
use crate::app::shell_wol::lobby::WolJoined;
use crate::app::shell_wol::{
    SERVER_STATUS_READY, WolApiStatus, WolLoginAccepted, WolProvider, WolServer, WolServerList,
    WolServiceEvent, WolServiceRequest, service_event,
};
use crate::ui::chat_translation::{ChatLanguage, ChatTranslation};
use crate::ui::shell::geom::RectPx;
use crate::ui::wol_chat::WolChatLine;
use crate::ui::wol_lobby::{
    self as lobby_ui, CHANNEL_KEYED, GAME_CHANNEL, GAME_LIMIT_800, LOBBY_KEY, MEMBER_OPERATOR,
    MEMBER_VOICE, WolChannel, WolMember,
};
use crate::ui::wol_login::{BACK, DIALOG as LOGIN_DIALOG, LOGIN, WolCredential};
use crate::ui::wol_options::WolPreferences;
use vera20k_gui::Prop;

mod buddies;
mod driver;
mod lobby;
mod my_info;
mod options;
mod pages;
mod quick_match;
mod test_service;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WolTarget {
    /// `0x10F` settled after a controlled server list, empty fields.
    LoginSteady,
    /// VERA Online's initial guest/account chooser; nothing is sent.
    ModernLoginPage,
    /// Its sign-in form with a username and password typed and Remember
    /// password ticked; nothing is sent.
    ModernLogin,
    /// The account form with only a username typed: Sign In stays disabled.
    ModernUsername,
    /// Create Account's popup, filled in; nothing is sent.
    ModernCreateAccount,
    /// Create Account's mailed-code popup, its request answered in place.
    ModernAccountCode,
    /// Forgot Password's code and new password popup, its username's code
    /// request answered in place.
    ModernForgotPassword,
    /// Delete Account's username or email and password popup, filled in; nothing is
    /// sent.
    ModernDeleteAccount,
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
    /// Hovered games: a retail map's description, then the random map's
    /// `TXT_RANDOM_MAP_DESCRIPTION`, after the ping (`0x00780D90`).
    LobbyGameTooltip,
    /// Ladder and high-score records: the rank text, the rank digits, the
    /// co-op image and the badges in the player rows (`0x00783C80`).
    LobbyPlayerColumns,
    /// The rank sort header, then a ladder record that moves its user
    /// (`0x00782320` from the ladder sink).
    LobbyRankSort,
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
    /// C2 with a joined guest: its row and the join's requests.
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
    /// C9's host leaves VERA's lobby room and the guest's game takes it
    /// over (docs/lobby-protocol.md "Host handover").
    HostHandover,
    /// VERA's online room: the host's Lobby shows the public lobby while the
    /// room goes on hidden, seating who comes and letting go of who leaves,
    /// and Back to Room shows it as it is now; a second visit outlives the
    /// room's close (docs/lobby-protocol.md "Visiting the lobby").
    LobbyVisit,
    /// A started WOL match left through FF/B6: the chat lobby rejoined.
    AbortReturn,
    /// A started WOL match's FF menu after a physical speed-trackbar jump.
    InternetMenu,
    /// `0x113` Back: the channel left and a fresh `0x10E` settled.
    LobbyBack,
    /// The offline test service: login to a settled 0x113.
    TestServiceLobby,
    /// Modern lobby presentation with sealed local fixture data, no
    /// credentials, Tanya's row hovered.
    ModernLobby,
    /// One open room and one running match from the list service, with the
    /// pointer resting on the requested row. Both retain the same two rows.
    ModernLobbySummary { watch_teams: Option<usize> },
    /// The same, then Create Game: `0xC3` on Red Alert 2's box art, its
    /// Ranked game box's help hovered.
    ModernCreate,
    /// The same, then a click on Tanya: VERA's player card with her badge,
    /// country and season record, her season row's help hovered.
    ModernPlayerCard,
    /// The same, then a click on your own name: Change Country picks
    /// Australia, which VERA.INI keeps and the lobby is sent; the list is
    /// open again over the card, Australia lit.
    ModernChangeCountry,
    /// The same, then OK: VERA's C2, whose Game Options toggles Crates.
    ModernRoomOptions,
    /// VERA's C2 again, whose Teams box sets two teams: the Alamo's two
    /// seats make one a team, the host on Team A and the guest on Team B.
    /// Team Colors then gives Team A a warm colour and Team B a cool one.
    ModernTeams,
    /// VERA's C2 opens two spectator rows past its players and the stand-in
    /// guest moves to the first; the host seats them, the join queue seats
    /// them again, and the lobby's answer gives them a badge, which their
    /// row draws and the status line names under the pointer.
    ModernSpectators,
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
    /// The modern lobby's Options over the test service: `0x2BC` settled
    /// with Play Music alone, help under it.
    ModernOptions,
    /// The modern lobby's Options: the translation list picks Simplified
    /// Chinese and OK saves it; the reopened page shows it, the help under
    /// the list.
    ModernOptionsTranslation,
    /// The same, then the translation list open over the page.
    ModernOptionsTranslationList,
    /// `0x2BC` Play Music unchecked: the preview queues -3, nothing saved.
    OptionsMusicOff,
    /// Pages and Buddies toggled, then Cancel: the lobby again, applied
    /// unchanged, VERA.INI untouched.
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
    /// Test service: Join on Boris's tournament game: C9 holds two rows;
    /// after Accept, host options without a preview take it back.
    GuestTournamentRows,
    /// Test service: Create with the clan box (the user's squad): C2's
    /// lockout holds Short Game on, the team column is hidden.
    ClanLockout,
    /// The same, then Start with the stand-in guest in no squad:
    /// `TXT_CLAN_NOSTART`.
    ClanNoStart,
    /// The same, then Choose Map: no FreeForAll, official maps only.
    ClanChooseMap,
    /// A clan game's match: `[0xB779C4]` = 2, Start Game Now's clan lists
    /// and FixedAlliance reach the sim, an Ally request is refused, and the
    /// peer's EXIT kills its House (DestroyConnection `0x005DA8F7`).
    ClanLeaver,
    /// A clan game's B5 Abort opens the Surrender box (state 2).
    ClanSurrender,
    /// Its OK queues the player's DESTRUCT and EXIT.
    ClanSurrenderOk,
    /// Test service: welcome Quick Match, login; the stand-in matchbot's
    /// match ends in "Scenarios don't match" over `0x130`.
    QmTestService,
    /// Then the box's OK: the logged-out `0x10E`.
    QmTestServiceOk,
    /// `0x117` after the Quick Match lobby join: the request line and a dot.
    QmRequesting,
    /// The matchbot's progress, fail and unknown texts as `0x117` lines.
    QmMatchbotLines,
    /// No chat lobby at all: `WOL:NoMatchingAgentFound`.
    QmNoAgent,
    /// Lobbies, none of them `Lob_40_`: `WOL:MatchingServerUnavailable`.
    QmServerUnavailable,
    /// Stage 4's timeout: the `WOL:MatchTimeout` box.
    QmTimeout,
    /// Then its OK: the logged-out `0x10E`.
    QmTimeoutOk,
    /// `0x117` Cancel: the logged-out `0x10E`.
    QmCancel,
    /// The matchbot's `Exit`: the `WOL:MatchFailExit` box.
    QmExit,
    /// `Start` naming the user as host: the game channel is created and
    /// `0x117` waits for the guest.
    QmHostWaiting,
    /// `Start` naming the user as guest: the host's game found and joined,
    /// the host's options accepted.
    QmGuestJoined,
    /// The host's guest joins and accepts: StartGame, OnGameStart, the
    /// verify, then the first loading frame.
    QmHostStartLoading,
    /// That match left through B5/B6: the postmatch `0x122`.
    QmPostmatch,
    /// Then `0x122` Yes: the opponent added, the logged-out `0x10E`.
    QmPostmatchYes,
    /// Quick Co-op: the host's guest joins; Cooperative `+0x20` replaces
    /// the matchbot's map, `+0x94` publishes C0; the start reaches loading.
    QcHostStartLoading,
    /// Quick Co-op: the host's C0, then its options, accepted at once.
    QcGuestJoined,
}

impl WolTarget {
    /// Choose the diagnostic backend before the native welcome prefill runs.
    pub(in crate::app::diagnostics::shell_capture) fn welcome_provider(self) -> WolProvider {
        if self.online_login() {
            WolProvider::Online
        } else if self.test_service() {
            WolProvider::TestService
        } else {
            WolProvider::Controlled
        }
    }

    /// The online login page and its account popups, on the online provider
    /// with nothing sent.
    pub(in crate::app::diagnostics::shell_capture) fn online_login(self) -> bool {
        matches!(
            self,
            Self::ModernLoginPage
                | Self::ModernLogin
                | Self::ModernUsername
                | Self::ModernCreateAccount
                | Self::ModernAccountCode
                | Self::ModernForgotPassword
                | Self::ModernDeleteAccount
        )
    }

    /// The login is accepted and the custom lobby runs.
    fn lobby(self) -> bool {
        !matches!(
            self,
            Self::LoginSteady
                | Self::ModernLoginPage
                | Self::ModernLogin
                | Self::ModernUsername
                | Self::ModernCreateAccount
                | Self::ModernAccountCode
                | Self::ModernForgotPassword
                | Self::ModernDeleteAccount
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

    /// Test service: Create with the clan box.
    fn clan(self) -> bool {
        matches!(
            self,
            Self::ClanLockout | Self::ClanNoStart | Self::ClanChooseMap
        )
    }

    /// A controlled clan game with the guest in another squad, played.
    fn clan_match(self) -> bool {
        matches!(
            self,
            Self::ClanLeaver | Self::ClanSurrender | Self::ClanSurrenderOk
        )
    }

    /// The stand-in host whose game the C9 targets join.
    fn guest_host(self) -> &'static str {
        if self == Self::GuestTournamentRows {
            "Boris"
        } else {
            "Tanya"
        }
    }

    /// Test service: C9 in Tanya's game with the host's options.
    fn guest_service(self) -> bool {
        matches!(
            self,
            Self::GuestHostOptions
                | Self::GuestHostLeft
                | Self::GuestKicked
                | Self::GuestUserKicked
                | Self::GuestTournamentRows
        )
    }

    /// The production test service answers instead of the capture.
    fn test_service(self) -> bool {
        self.buddy()
            || self.guest_service()
            || matches!(
                self,
                Self::TestServiceLobby
                    | Self::ModernLobby
                    | Self::ModernLobbySummary { .. }
                    | Self::ModernCreate
                    | Self::ModernPlayerCard
                    | Self::ModernChangeCountry
                    | Self::ModernRoomOptions
                    | Self::LobbyVisit
                    | Self::ModernTeams
                    | Self::ModernSpectators
                    | Self::ModernOptions
                    | Self::ModernOptionsTranslation
                    | Self::ModernOptionsTranslationList
                    | Self::TestServiceCreate
                    | Self::TestServiceJoin
                    | Self::GuestChoices
                    | Self::GuestObserver
                    | Self::GuestKick
                    | Self::ScenariosMismatch
                    | Self::MismatchOk
                    | Self::ClanLockout
                    | Self::ClanNoStart
                    | Self::ClanChooseMap
                    | Self::PageDialog
                    | Self::PageFindStatus
                    | Self::PageUserGone
                    | Self::SetupPage
                    | Self::PageReceived
                    | Self::PageReceivedReply
                    | Self::PageReceivedBack
                    | Self::QmTestService
                    | Self::QmTestServiceOk
            )
    }

    /// Welcome's Quick Match runs the runner `0x0079FC40`.
    fn quick_match(self) -> bool {
        matches!(
            self,
            Self::QmTestService
                | Self::QmTestServiceOk
                | Self::QmRequesting
                | Self::QmMatchbotLines
                | Self::QmNoAgent
                | Self::QmServerUnavailable
                | Self::QmTimeout
                | Self::QmTimeoutOk
                | Self::QmCancel
                | Self::QmExit
                | Self::QmHostWaiting
                | Self::QmGuestJoined
                | Self::QmHostStartLoading
                | Self::QmPostmatch
                | Self::QmPostmatchYes
                | Self::QcHostStartLoading
                | Self::QcGuestJoined
        )
    }

    /// Welcome's Quick Co-op (`[0xA8B244]` 2) runs the same runner.
    pub(super) fn quick_coop(self) -> bool {
        matches!(self, Self::QcHostStartLoading | Self::QcGuestJoined)
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
            Self::JoinAccepted | Self::GuestAccept | Self::GuestBack | Self::HostHandover
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
                | Self::IngameConnectionLost
                | Self::IngamePage
                | Self::IngamePageCancel
                | Self::AbortRelogin
                | Self::VerifyDialog
                | Self::ClanLeaver
                | Self::ClanSurrender
                | Self::ClanSurrenderOk
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
                | Self::IngameConnectionLost
                | Self::IngamePage
                | Self::IngamePageCancel
                | Self::AbortRelogin
                | Self::ClanLeaver
                | Self::ClanSurrender
                | Self::ClanSurrenderOk
                | Self::QmPostmatch
                | Self::QmPostmatchYes
        )
    }
}

pub(crate) const WOL_CHECKPOINTS: &[(&str, WolTarget)] = &[
    ("wol-0x10f-steady", WolTarget::LoginSteady),
    ("wol-modern-login-page", WolTarget::ModernLoginPage),
    ("wol-modern-login", WolTarget::ModernLogin),
    ("wol-modern-username", WolTarget::ModernUsername),
    ("wol-modern-create-account", WolTarget::ModernCreateAccount),
    ("wol-modern-account-code", WolTarget::ModernAccountCode),
    (
        "wol-modern-forgot-password",
        WolTarget::ModernForgotPassword,
    ),
    ("wol-modern-delete-account", WolTarget::ModernDeleteAccount),
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
    ("wol-0x113-game-tooltip", WolTarget::LobbyGameTooltip),
    ("wol-0x113-player-columns", WolTarget::LobbyPlayerColumns),
    ("wol-0x113-rank-sort", WolTarget::LobbyRankSort),
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
    ("wol-c9-host-handover", WolTarget::HostHandover),
    ("wol-modern-lobby-visit", WolTarget::LobbyVisit),
    ("wol-abort-return", WolTarget::AbortReturn),
    ("wol-0xff-menu", WolTarget::InternetMenu),
    ("wol-test-service-lobby", WolTarget::TestServiceLobby),
    ("wol-modern-lobby", WolTarget::ModernLobby),
    (
        "wol-modern-game-summary",
        WolTarget::ModernLobbySummary { watch_teams: None },
    ),
    (
        "wol-modern-watch-summary",
        WolTarget::ModernLobbySummary {
            watch_teams: Some(2),
        },
    ),
    (
        "wol-modern-many-team-summary",
        WolTarget::ModernLobbySummary {
            watch_teams: Some(15),
        },
    ),
    ("wol-modern-create", WolTarget::ModernCreate),
    ("wol-modern-player-card", WolTarget::ModernPlayerCard),
    ("wol-modern-change-country", WolTarget::ModernChangeCountry),
    ("wol-modern-room-options", WolTarget::ModernRoomOptions),
    ("wol-modern-teams", WolTarget::ModernTeams),
    ("wol-modern-spectators", WolTarget::ModernSpectators),
    ("wol-test-service-create", WolTarget::TestServiceCreate),
    ("wol-test-service-join", WolTarget::TestServiceJoin),
    ("wol-c2-guest-choices", WolTarget::GuestChoices),
    ("wol-c2-guest-observer", WolTarget::GuestObserver),
    ("wol-c2-kick", WolTarget::GuestKick),
    ("wol-0x130-verify", WolTarget::VerifyDialog),
    ("wol-0x130-scenarios-mismatch", WolTarget::ScenariosMismatch),
    ("wol-0x130-mismatch-ok", WolTarget::MismatchOk),
    ("wol-0x2bc-steady", WolTarget::OptionsSteady),
    ("wol-modern-options", WolTarget::ModernOptions),
    (
        "wol-modern-options-translation",
        WolTarget::ModernOptionsTranslation,
    ),
    (
        "wol-modern-options-translation-list",
        WolTarget::ModernOptionsTranslationList,
    ),
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
    ("wol-c9-tournament-rows", WolTarget::GuestTournamentRows),
    ("wol-c2-clan-lockout", WolTarget::ClanLockout),
    ("wol-c2-clan-nostart", WolTarget::ClanNoStart),
    ("wol-choose-map-clan-filter", WolTarget::ClanChooseMap),
    ("wol-clan-leaver-dies", WolTarget::ClanLeaver),
    ("wol-clan-surrender", WolTarget::ClanSurrender),
    ("wol-clan-surrender-ok", WolTarget::ClanSurrenderOk),
    ("wol-qm-test-service", WolTarget::QmTestService),
    ("wol-qm-test-service-ok", WolTarget::QmTestServiceOk),
    ("wol-0x117-requesting", WolTarget::QmRequesting),
    ("wol-0x117-matchbot-lines", WolTarget::QmMatchbotLines),
    ("wol-qm-no-agent", WolTarget::QmNoAgent),
    ("wol-qm-server-unavailable", WolTarget::QmServerUnavailable),
    ("wol-qm-timeout", WolTarget::QmTimeout),
    ("wol-qm-timeout-ok", WolTarget::QmTimeoutOk),
    ("wol-0x117-cancel", WolTarget::QmCancel),
    ("wol-qm-exit", WolTarget::QmExit),
    ("wol-qm-host-waiting", WolTarget::QmHostWaiting),
    ("wol-qm-guest-joined", WolTarget::QmGuestJoined),
    ("wol-qm-host-start-loading", WolTarget::QmHostStartLoading),
    ("wol-0x122-postmatch", WolTarget::QmPostmatch),
    ("wol-0x122-yes", WolTarget::QmPostmatchYes),
    ("wol-qc-host-start-loading", WolTarget::QcHostStartLoading),
    ("wol-qc-guest-joined", WolTarget::QcGuestJoined),
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
    /// Options OK with a translation language: waiting for the lobby.
    TranslationReturn,
    /// Options again after the language was saved.
    TranslationReopened,
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
    /// A clan game's peer EXIT was delivered at `tick`.
    ClanLeaving {
        tick: u64,
    },
    /// The Quick Match runner's own steps ([`quick_match`]).
    Qm(quick_match::QmPhase),
}

pub(super) struct WolCapture {
    target: WolTarget,
    phase: WolPhase,
    route: Vec<Value>,
    /// Setup Back or the match return rejoined the chat lobby.
    rejoined: bool,
    abort_input: Option<abort_return::AbortReturnInput>,
    exit_consumed: bool,
    /// VERA.INI and the live WOL preferences when Options was pressed.
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

    /// Your own card's Change Country: the list opens at the login's United
    /// States (2), a pick of another shown country is kept and sent, and the
    /// list then opens again at that row.
    fn change_country(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::app::shell_wol::player_card;
        use crate::ui::wol_lobby::CARD_COUNTRY_ROWS;
        player_row_click(state, NICKNAME)?;
        ensure!(
            Self::lobby(state)?
                .ui
                .card
                .as_ref()
                .is_some_and(|card| card.name == NICKNAME),
            "a click on your own name did not open your card"
        );
        let message = player_card::layout(state).message;
        let countries = |state: &AppState| {
            player_card::view(state).and_then(|view| view.countries.map(|list| (view.rows, list)))
        };
        // The open list's row for `locale` and the top row it should open at.
        let opens_at = |state: &AppState, locale: i32| -> Result<(usize, usize)> {
            let rows = &Self::lobby(state)?
                .ui
                .card
                .as_ref()
                .context("your card")?
                .countries;
            let row = rows
                .iter()
                .position(|row| row.locale == locale)
                .context("the country's row")?;
            Ok((row, row.min(rows.len().saturating_sub(CARD_COUNTRY_ROWS))))
        };
        welcome_click(state, message);
        let (_, list) = countries(state).context("Change Country did not drop the country list")?;
        let (lit, top) = opens_at(state, 2)?;
        ensure!(
            list.hot_row == Some(lit) && list.popup.first == top,
            "the list opened at row {:?}, top {}, not at the United States",
            list.hot_row,
            list.popup.first
        );
        // The first shown row of another country: where the United States
        // sits depends on the language's names.
        let (row, locale) = {
            let card = Self::lobby(state)?.ui.card.as_ref().context("your card")?;
            (list.popup.first..list.popup.first + CARD_COUNTRY_ROWS)
                .filter_map(|row| card.countries.get(row).map(|country| (row, country.locale)))
                .find(|&(_, locale)| locale != 2)
                .context("every shown row is the United States")?
        };
        welcome_click(state, list.popup.row_rect(row).context("the picked row")?);
        ensure!(countries(state).is_none(), "the pick left the list open");
        ensure!(
            state.persistence.options_profile.online_locale == locale,
            "VERA.INI did not keep {locale}"
        );
        let requests = self.requests(state, frame)?;
        ensure!(
            requests.contains(&WolServiceRequest::SetLocale(locale)),
            "the pick sent {requests:?}"
        );
        self.event(frame, json!({"country kept and sent": locale}));
        welcome_click(state, message);
        let (rows, list) = countries(state).context("Change Country did not open again")?;
        let (lit, top) = opens_at(state, locale)?;
        ensure!(
            lit == row && list.hot_row == Some(row) && list.popup.first == top,
            "the list opened again at row {:?}, top {}",
            list.hot_row,
            list.popup.first
        );
        let card = Self::lobby(state)?.ui.card.as_ref().context("your card")?;
        let picked = &card.countries.get(row).context("the picked row")?.text;
        ensure!(
            rows.iter().any(|(_, value)| value == picked),
            "the card shows {rows:?}"
        );
        Ok(())
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
        // The Quick Match runner's lobby is never shown, but `0x117` over
        // it paints the status line unless a box or the wait covers it.
        let quick_match_page = crate::app::shell_wol::quick_match::dialog(state).is_some()
            && state
                .frontend
                .wol
                .as_ref()
                .and_then(|visit| visit.lobby())
                .is_some_and(|lobby| lobby.message().is_none() && lobby.wait.is_none());
        let backdrop = !quick_match_page
            && state.frontend.wol.as_ref().is_some_and(|visit| {
                matches!(
                    visit.page,
                    crate::app::shell_wol::WolPage::Backdrop { .. }
                        | crate::app::shell_wol::WolPage::Message { .. }
                ) || visit
                    .lobby()
                    .is_some_and(|lobby| !lobby.shown || lobby.covered || lobby.ui.popup.is_some())
            })
            || state.frontend.shell_route == ShellRoute::NetworkSetup;
        if let Some(menu) = match self.target {
            WolTarget::InternetMenu => Some(crate::ui::pause_menu::InGameMenuState::Menu),
            WolTarget::IngameConnectionLost | WolTarget::IngamePageCancel => {
                Some(crate::ui::pause_menu::InGameMenuState::Closed)
            }
            WolTarget::IngamePage => Some(crate::ui::pause_menu::InGameMenuState::Page),
            WolTarget::ClanSurrender => Some(crate::ui::pause_menu::InGameMenuState::Surrender),
            WolTarget::ClanLeaver => Some(crate::ui::pause_menu::InGameMenuState::Closed),
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
                    .runtime()
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
        // The guest's badge help types itself out in the room's own status
        // line.
        let room_line = self.target != WolTarget::ModernSpectators
            || state
                .frontend
                .multiplayer_setup
                .as_ref()
                .is_some_and(|ui| ui.statics.is_terminal());
        Ok(self.phase == WolPhase::Settle(0)
            && state.frontend.shell_exit.is_none()
            && (backdrop || state.frontend.shell_status_line.is_terminal())
            && room_line
            && state.frontend.shell_first_paint_slide.is_none())
    }

    pub(super) fn receipt(&self, state: &AppState) -> Value {
        let visit = state.frontend.wol.as_ref();
        json!({
            "target": format!("{:?}", self.target),
            "route": self.route,
            "service_boundary": if self.target.online_login() {
                "ordinary online login page; visual capture only, no authenticated backend claim"
            } else { "controlled decoded WOL results; no WOLAPI, transport or backend claim" },
            "shell_route": format!("{:?}", state.frontend.shell_route),
            "mode": visit.and_then(|visit| visit.mode()).map(|mode| mode.native()),
            "authenticated": visit.is_some_and(|visit| visit.authenticated()),
            "wolinfo": state.platform.game_config.as_ref().and_then(|config| {
                std::fs::read_to_string(config.paths.settings_dir().join(WOLINFO_INI)).ok()
            }),
            "login": visit.and_then(|visit| visit.login()).map(|login| {
                use crate::ui::wol_login::{FORGET, NICKNAME, PASSWORD, REMEMBER};
                let ui = &state.frontend.shell_controller;
                let flag = |id, prop, default| ui.flag(LOGIN_DIALOG, id, prop, default);
                json!({
                    "nickname": ui.text(LOGIN_DIALOG, NICKNAME),
                    "password_units": ui.text(LOGIN_DIALOG, PASSWORD).encode_utf16().count(),
                    "login_enabled": flag(LOGIN, Prop::Enabled, true),
                    "forget_enabled": flag(FORGET, Prop::Enabled, true),
                    "remember_enabled": flag(REMEMBER, Prop::Enabled, true),
                    "remember_checked": flag(REMEMBER, Prop::Checked, false),
                    "focused": ui.focused_control(LOGIN_DIALOG),
                    "entry": format!("{:?}", login.policy),
                    "popup": login.account.as_ref().map(|form| format!("{:?}", form.kind)),
                })
            }),
        })
    }
}

/// The controlled guest's House.
fn clan_peer_house(state: &AppState) -> Result<crate::sim::intern::InternedId> {
    let sim = &state
        .match_state
        .sim_runtime
        .runtime()
        .context("the clan match runtime")?
        .simulation;
    state
        .match_state
        .session
        .network_source_owner(1)
        .and_then(|owner| sim.interner().get(&owner))
        .context("the clan peer's House")
}

/// The user's and the controlled guest's squads in a clan game.
const CLAN_SQUAD: i32 = 7;
const RIVAL_SQUAD: i32 = 9;

fn squad_member(name: &str, flags: u32, squad: i32, tag: &str) -> WolMember {
    WolMember {
        squad,
        ..member(name, flags, tag)
    }
}

fn member(name: &str, flags: u32, tag: &str) -> WolMember {
    WolMember {
        flags,
        squad: 0,
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

/// An open game with a free observer slot on a retail map, a full one on
/// the random map, one with other rules (`hash + 1`) and a keyed one
/// limited to 800x600. `hash` is the local INI hash, which the others
/// publish.
fn games(hash: i32, map: &str) -> Vec<WolChannel> {
    let game = |name: &str, users, max_users, ex_info: String, ping| WolChannel {
        kind: GAME_CHANNEL,
        max_users,
        users,
        ping,
        name: name.into(),
        ex_info,
        ..Default::default()
    };
    let mut keyed = game("Delta's_game", 2, 6, format!("25,{hash},0,0,0,d.map"), 80);
    keyed.flags = CHANNEL_KEYED;
    keyed.game_flags = GAME_LIMIT_800;
    vec![
        game("Alpha's_game", 2, 8, format!("25,{hash},1,0,1,{map}"), 45),
        game(
            "Bravo's_game",
            3,
            4,
            format!("25,{hash},1,0,0,RandMap.Sed"),
            120,
        ),
        game(
            "Charlie's_game",
            1,
            8,
            format!("25,{},0,0,0,c.map", hash.wrapping_add(1)),
            -1,
        ),
        keyed,
    ]
}

/// The games with the local hash, Alpha's naming the first scenario record.
fn local_games(state: &AppState) -> Vec<WolChannel> {
    games(
        crate::app::shell_wol::lobby::local_ini_hash(state).unwrap_or(0),
        &first_record(state).0,
    )
}

/// The first scenario record's file and description.
fn first_record(state: &AppState) -> (String, String) {
    state
        .frontend
        .scenario_catalog
        .records()
        .first()
        .map(|record| (record.file_name.clone(), record.display_name.clone()))
        .unwrap_or_default()
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
        ShellSlideKind::NetworkSetup(crate::app::shell_network_setup::setup_dialog(
            state,
            host,
            crate::ui::multiplayer_setup::SetupFlavor::Wol,
        )),
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

fn settings_path(state: &AppState) -> Result<std::path::PathBuf> {
    let config = state
        .platform
        .game_config
        .as_ref()
        .context("capture game config")?;
    Ok(crate::app::persistence::profile_file::path(
        config.paths.settings_dir(),
    ))
}

fn read_settings(state: &AppState) -> Result<Option<Vec<u8>>> {
    let path = settings_path(state)?;
    match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

fn restore_settings(state: &AppState, bytes: Option<Vec<u8>>) -> Result<()> {
    let path = settings_path(state)?;
    match bytes {
        Some(bytes) => std::fs::write(&path, bytes),
        None => std::fs::remove_file(&path),
    }
    .with_context(|| format!("restore {}", path.display()))
}

/// The translation list's face where a press opens it: the arrow at its right.
fn translate_list_arrow(layout: &crate::ui::wol_options::WolOptionsLayout) -> Result<RectPx> {
    let face = layout
        .rect(crate::ui::wol_options::TRANSLATE)
        .context("the translation list")?;
    Ok(RectPx::new(face.x + face.w - 12, face.y, 4, face.h))
}

/// What the translation movie's clicks choose.
fn translated_lobby_only() -> ChatTranslation {
    ChatTranslation {
        language: Some(ChatLanguage::SimplifiedChinese),
        in_matches: false,
    }
}

fn lobby_button(state: &mut AppState, layout: &lobby_ui::WolLobbyLayout, id: u16) -> Result<()> {
    welcome_click(state, layout.page.button_rect(id).context("0x113 button")?);
    Ok(())
}

/// The online page's chat view through its view button, so the chat and the
/// player list show.
fn show_lobby_chat(state: &mut AppState) -> Result<()> {
    let shows_chat = |state: &AppState| {
        state
            .frontend
            .wol
            .as_ref()
            .and_then(|visit| visit.lobby())
            .is_some_and(|lobby| lobby.ui.shows_chat())
    };
    if !shows_chat(state) {
        let layout = crate::app::shell_wol::lobby::layout(state);
        lobby_button(state, &layout, lobby_ui::VIEW)?;
    }
    ensure!(shows_chat(state), "the view button did not show the chat");
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
    let geometry = lobby.ui.shown_geometry(
        crate::ui::wol_lobby::GAMES,
        layout
            .control_rect(crate::ui::wol_lobby::GAMES)
            .unwrap_or_default(),
        lobby.ui.game_rows.len(),
        layout.page.scale,
    );
    let rect = geometry.row(row - lobby.ui.list_top(crate::ui::wol_lobby::GAMES));
    welcome_click(state, rect);
    Ok(())
}

/// A click on the player list's row of `name`, in the chat view.
fn player_row_click(state: &mut AppState, name: &str) -> Result<()> {
    show_lobby_chat(state)?;
    let rect = player_row_rect(state, name)?;
    welcome_click(state, rect);
    Ok(())
}

/// The modern player list's row of `name`.
fn player_row_rect(state: &AppState, name: &str) -> Result<RectPx> {
    let layout = crate::app::shell_wol::lobby::layout(state);
    let lobby = state
        .frontend
        .wol
        .as_ref()
        .and_then(|visit| visit.lobby())
        .context("the lobby runner")?;
    let row = lobby
        .ui
        .player_rows
        .iter()
        .position(|row| row.name == name)
        .context("the player's row")?;
    let geometry = lobby.ui.shown_geometry(
        crate::ui::wol_lobby::PLAYERS,
        layout
            .control_rect(crate::ui::wol_lobby::PLAYERS)
            .unwrap_or_default(),
        lobby.ui.player_rows.len(),
        layout.page.scale,
    );
    Ok(geometry.row(row - lobby.ui.list_top(crate::ui::wol_lobby::PLAYERS)))
}

/// Three IRC records; the preferred-server draw left record 1 selected.
/// `wolinfoMD.ini` beside the isolated VERA.INI.
fn info_file(state: &AppState) -> Result<WolInfoFile> {
    let config = state
        .platform
        .game_config
        .as_ref()
        .context("capture game config")?;
    Ok(WolInfoFile::in_directory(config.paths.settings_dir()))
}

/// The target's persona file, only in the separately marked settings root.
fn write_wolinfo(state: &AppState, target: WolTarget) -> Result<()> {
    let root = super::lan::LanCapture::isolated_settings_root(state)?;
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

/// The test service's page back from `name`, as the page dialog shows it
/// (`(name) ` before the stand-in's line, in the player's language).
fn test_page_back(name: &str) -> String {
    format!("({name}) {}", crate::i18n::tr("VERA:TestPageReply"))
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

/// The open `0x10F` and the shell's widgets it stands on.
fn login_mut(
    state: &mut AppState,
) -> Result<(
    &mut crate::ui::wol_login::WolLoginState,
    &mut crate::ui::shell::controller::DialogController,
)> {
    let frontend = &mut state.frontend;
    match frontend.wol.as_mut().map(|visit| &mut visit.page) {
        Some(crate::app::shell_wol::WolPage::Login(login)) => {
            Ok((login, &mut frontend.shell_controller))
        }
        _ => bail!("0x10F is not open"),
    }
}

/// Type into the account popup's fields from the focused one, Tab between.
fn account_type(state: &mut AppState, values: &[&str]) {
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            App::handle_wol_key(state, Some(winit::keyboard::KeyCode::Tab), true, None);
        }
        App::handle_wol_key(state, None, true, Some(value));
    }
}

fn login_click(state: &mut AppState, rect: RectPx) {
    // The combo face's left part is its NewEdit, clear of the arrow.
    let rect = RectPx::new(rect.x, rect.y, rect.w / 2, rect.h);
    welcome_click(state, rect);
}

fn setup_view(state: &AppState) -> Result<crate::ui::multiplayer_setup::SetupView> {
    setup::view(state).context("C2/C9 view")
}

/// Row `row`'s combo in `column` and the page row it stands on: VERA's room
/// lists its rows under team headers, so a page row is not a seat.
fn row_combo(
    view: &crate::ui::multiplayer_setup::SetupView,
    column: crate::ui::multiplayer_setup::SetupColumn,
    row: u8,
) -> Option<(usize, &crate::ui::multiplayer_setup::SetupCombo)> {
    view.combos.iter().find_map(|combo| {
        crate::ui::multiplayer_setup::SetupColumn::decode(combo.id)
            .filter(|(candidate, _)| *candidate == column && combo.row == row)
            .map(|(_, page_row)| (page_row, combo))
    })
}

/// The joined game's host options again, naming a version of its map this
/// guest cannot verify (another digest).
fn unverified_host_options(state: &AppState) -> Result<NetworkSetupEvent> {
    let launch = state
        .match_state
        .session
        .network_setup()
        .context("C9 setup")?
        .clone();
    let file = launch.selected_map_file.clone().context("host map")?;
    let mut map_metadata = crate::app::shell_network_setup::local_map_metadata(state, &file)?;
    map_metadata.digest = "another version".into();
    Ok(NetworkSetupEvent::HostSetup(
        crate::app::shell_network_setup::HostOptions {
            launch,
            map_metadata,
            vacancies: [crate::app::match_runtime::session::NetworkVacancy::Open;
                crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT],
            observer_row: None,
            cooperative_difficulty: 0,
            seed: state.match_state.session.network_start_seed(),
        },
    ))
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

/// Presses the Join or Watch on the header of `group`'s box.
fn press_header(
    state: &mut AppState,
    group: crate::ui::multiplayer_setup::SetupGroup,
) -> Result<()> {
    let view = setup_view(state)?;
    let row = view
        .headers
        .iter()
        .position(|header| header.as_ref().is_some_and(|header| header.group == group))
        .with_context(|| format!("{group:?}'s header"))?;
    let rect = crate::app::input::multiplayer_setup::layout(state).header_controls[row].0;
    setup_click(state, rect);
    Ok(())
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

/// What `0x114`'s widgets show.
struct MyInfoShown {
    data: Vec<i32>,
    server_face: Option<String>,
    nicknames: Vec<String>,
    slot: i32,
    auto_login: bool,
    auto_login_enabled: bool,
    match_same_res: bool,
    forget_enabled: bool,
    play_buddy_enabled: bool,
    texts: Vec<(u16, String)>,
}

fn my_info_shown(state: &AppState) -> Result<MyInfoShown> {
    use crate::app::shell_wol::my_info;
    use crate::ui::wol_my_info::*;
    let page = my_info::page(state).context("0x114")?;
    let root = my_info::root(state).context("0x114's widgets")?;
    let gui = state.frontend.shell_controller.gui();
    let flag = |id: u16, prop: Prop| {
        crate::ui::shell::page::get(gui, root, id, prop) != Some(&vera20k_gui::Value::Bool(false))
    };
    let server = selected(gui, root, SERVER).and_then(|row| page.combos[1].get(row));
    Ok(MyInfoShown {
        data: COMBOS.map(|id| page.data_or(gui, root, id, -99)).to_vec(),
        server_face: server.map(|row| row.text.clone()),
        nicknames: page.combos[0].iter().map(|row| row.text.clone()).collect(),
        slot: page.slot(gui, root),
        auto_login: crate::ui::shell::page::checked(gui, root, AUTO_LOGIN),
        auto_login_enabled: flag(AUTO_LOGIN, Prop::Enabled),
        match_same_res: crate::ui::shell::page::checked(gui, root, MATCH_SAME_RES),
        forget_enabled: flag(FORGET, Prop::Enabled),
        play_buddy_enabled: flag(PLAY_BUDDY, Prop::Enabled),
        texts: page.texts.clone(),
    })
}

fn layout_button(state: &AppState, id: u16) -> Result<RectPx> {
    crate::app::shell_wol::my_info::layout(state)
        .page
        .button_rect(id)
        .context("0x114 button")
}
