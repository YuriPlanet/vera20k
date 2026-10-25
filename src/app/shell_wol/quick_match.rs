//! The Quick Match runner `0x0079FC40` (WOL mode 1) and its page `0x117`
//! (proc `0x007A2110`), from the login to a match or back to the welcome,
//! and the postmatch add-buddy query `0x122` (`0x007A2970`).
//!
//! The runner lists the chat lobbies, takes the first whose name starts
//! with `Lob_40_`, shows `0x117`, joins that lobby and, when its user list
//! arrives, privately asks its matchbot (the last channel operator) for a
//! match (`0x007A7B0D..0x007A7B3B`). The matchbot's ANSI
//! replies (`0x007A16B0`) print progress lines, fail, or name the two
//! players: the first creates a two-player game channel, the second finds
//! and joins it. The host applies the options the matchbot sent, the guest
//! accepts them by itself, and the host starts the game as soon as the
//! guest accepts; Start Game Now (`0x00794BA0`) then runs as for a custom
//! game. A stage machine with per-stage `time()` timeouts (`0x0077F900`)
//! moves a stalled match back to the lobby. No RNG is drawn on these paths.
//!
//! The runner's channel records (the lobby list, the users, the current and
//! pending channel) are the WOL globals [`super::lobby::WolLobby`] holds
//! with [`super::lobby::Runner::QuickMatch`]; its joins, creates and sinks
//! run there with the native mode 1-3 branches. The game channel's nodes
//! and the start are the network setup's, without C2/C9
//! ([`crate::app::shell_network_setup`]). Quick Co-op (mode 2) is a later
//! chain: the welcome keeps refusing it.

use std::time::{Duration, Instant};

use super::WolPage;
use super::lobby;
use super::service::{self, WolServiceRequest};
use crate::app::frontend::{shell_layout, shell_transition};
use crate::app::{App, AppState};
use crate::ui::lan_shell::LanChatLine;
use crate::ui::wol_lobby::{GAME_CHANNEL, MEMBER_OPERATOR, WolChannel};
use crate::ui::wol_quick_match::*;
use crate::ui::wol_shell::WolMode;

/// `0x0084A21C`: the Quick Match lobbies' prefix.
pub(crate) const QUICK_MATCH_PREFIX: &str = "Lob_40_";
/// `0x0084A22C`: the Quick Co-op lobbies' prefix (mode 2, `0x0079FD9E`).
pub(crate) const QUICK_COOP_PREFIX: &str = "Lob_39_";
/// `0x0084B698`: the key `0x00788800` joins a Quick Match lobby with (the
/// same text as the custom lobbies' key).
const QUICK_MATCH_KEY: &str = crate::ui::wol_lobby::LOBBY_KEY;
/// `SetTimer(dlg, 0, 2000)`: `0x005E2AC0`'s dots.
const DOT_PERIOD: Duration = Duration::from_millis(2000);
/// `0x007A1BB0`'s guest search: 30 list requests, 100 ms apart
/// (`0x007A1E35`, `0x007A1F27..0x007A1F42`).
const SEARCH_TRIES: u32 = 30;
const SEARCH_PAUSE: Duration = Duration::from_millis(100);

/// Transcript colours (`COLORREF`) the runner writes with.
const REQUEST_COLOR: u32 = 0xFF_8080;
const PROGRESS_COLOR: u32 = 0xFF_8080;
const FAIL_COLOR: u32 = 0x40_FFFF;
const ERROR_COLOR: u32 = 0x20_C0FF;
const FOUND_COLOR: u32 = 0xC0_FFFF;

/// The runner's stages `[0xB73A20]`.
pub(crate) mod stage {
    /// Failed; event 5 shows the failure box.
    pub(crate) const FAILED: u8 = 0;
    pub(crate) const IDLE: u8 = 1;
    /// Joining the Quick Match lobby.
    pub(crate) const JOIN_LOBBY: u8 = 2;
    /// In the lobby: the request goes out.
    pub(crate) const IN_LOBBY: u8 = 3;
    pub(crate) const REQUESTED: u8 = 4;
    /// The matchbot said `Working`.
    pub(crate) const WORKING: u8 = 5;
    pub(crate) const CREATING: u8 = 6;
    pub(crate) const SEARCHING: u8 = 7;
    /// In the game channel, waiting for the game to start.
    pub(crate) const WAITING: u8 = 9;
    /// Start Game Now has run (`0x00794BA0`).
    pub(crate) const STARTED: u8 = 11;
}

/// `[0xB77E14]`: why the runner failed; the box event 5 shows.
const FAILURE_NO_AGENT: u32 = 1;
const FAILURE_TIMEOUT: u32 = 2;

/// The runner's WOL globals. They outlive the runner: the opponent
/// `[0xB76AC4]` feeds the postmatch query after the match.
#[derive(Debug, Clone, Default)]
pub(crate) struct QuickMatchGlobals {
    /// `[0xB73A20]`.
    pub(crate) stage: u8,
    /// `[0xB77D70]` in seconds, `None` for -1.
    timeout: Option<u64>,
    /// `[0xB77A88]`: `time()` at the last stage write.
    stamp: u64,
    /// `[0xB77E14]`.
    failure: u32,
    /// `[0xB76AC4]`: the matched opponent, written only by `Start`.
    pub(crate) opponent: Option<String>,
    /// `0xB76230` while `[0xB77E40]` is set: pages are answered with
    /// `TXT_AWAY_QUICKMATCH`.
    away: Option<Vec<u16>>,
    /// The runner's Quick Match lobby, found at its entry.
    lobby: Option<WolChannel>,
    /// `[0xB73714]`: the matchbot's last `Start` payload, decoded.
    start: Option<Box<crate::app::shell_network_setup::HostOptions>>,
    /// The guest's search for the host's game.
    search: Option<GameSearch>,
}

#[derive(Debug, Clone)]
struct GameSearch {
    name: String,
    tries: u32,
    /// The next list request after the pause; `None` while one is out.
    next_at: Option<Instant>,
}

/// `0x117` while it exists.
#[derive(Debug, Clone)]
pub(crate) struct QuickMatchDialog {
    pub(crate) ui: WolQuickMatchPage,
    dot_at: Instant,
}

/// `time()`.
fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

impl QuickMatchGlobals {
    /// A stage write with its timeout and `time()` stamp.
    fn set(&mut self, stage: u8, timeout: Option<u64>) {
        self.stage = stage;
        self.timeout = timeout;
        self.stamp = now_seconds();
    }

    /// The pump `0x0077F900`'s test: a stage outside 0, 1 and 11 whose
    /// timeout has passed (`time() - stamp > timeout`, unsigned).
    fn expired(&self, now: u64) -> bool {
        !matches!(self.stage, stage::FAILED | stage::IDLE | stage::STARTED)
            && self
                .timeout
                .is_some_and(|timeout| now.wrapping_sub(self.stamp) > timeout)
    }

    /// `0x117`'s Cancel (event 0) during the guest's search: the list wait
    /// `0x0077F120` takes it and returns 0, so the search ends with stage 3
    /// (20 s, `0x007A1F5A..0x007A1FA6`) and the host's name for
    /// `WOL:MatchGameChannelNotFound`. Between tries the next try's
    /// `ResetEvent` sweep over the 18 events drops it. `None` outside a
    /// search; the runner then ends as usual.
    fn cancel_search(&mut self) -> Option<Option<String>> {
        let search = self.search.as_ref()?;
        if search.next_at.is_some() {
            return Some(None);
        }
        let host = search.name.trim_end_matches("'s_game").to_owned();
        self.search = None;
        self.failure = 0;
        self.set(stage::IN_LOBBY, Some(20));
        Some(Some(host))
    }

    /// When [`Self::expired`] turns true.
    fn deadline(&self, now: u64) -> Option<Instant> {
        if matches!(self.stage, stage::FAILED | stage::IDLE | stage::STARTED) {
            return None;
        }
        let due = self.stamp.checked_add(self.timeout? + 1)?;
        Some(Instant::now() + Duration::from_secs(due.saturating_sub(now)))
    }
}

/// The visit runs the Quick Match runner: Quick Match (mode 1) or Quick
/// Co-op (mode 2), which every runner path treats alike except the lobby
/// prefix, the game channel's tournament and the load's start points.
pub(crate) fn mode_active(state: &AppState) -> bool {
    state.frontend.wol.as_ref().is_some_and(|visit| {
        matches!(visit.mode, Some(WolMode::QuickMatch | WolMode::QuickCoop))
    })
}

/// `[0xA8B244] == 2`: Quick Co-op.
pub(crate) fn quick_coop(state: &AppState) -> bool {
    state
        .frontend
        .wol
        .as_ref()
        .is_some_and(|visit| visit.mode == Some(WolMode::QuickCoop))
}

fn globals(state: &AppState) -> Option<&QuickMatchGlobals> {
    Some(&state.frontend.wol.as_ref()?.quick_match)
}

fn globals_mut(state: &mut AppState) -> Option<&mut QuickMatchGlobals> {
    Some(&mut state.frontend.wol.as_mut()?.quick_match)
}

/// The runner's stage (`[0xB73A20]`).
pub(crate) fn stage(state: &AppState) -> u8 {
    globals(state).map_or(stage::IDLE, |globals| globals.stage)
}

fn set_stage(state: &mut AppState, stage: u8, timeout: Option<u64>) {
    if let Some(globals) = globals_mut(state) {
        globals.set(stage, timeout);
    }
}

/// The matchbot's options for the host (`[0xB73714]`).
pub(crate) fn start_options(
    state: &AppState,
) -> Option<crate::app::shell_network_setup::HostOptions> {
    globals(state)?.start.as_deref().cloned()
}

pub(crate) fn dialog(state: &AppState) -> Option<&QuickMatchDialog> {
    match &state.frontend.wol.as_ref()?.page {
        WolPage::QuickMatch(dialog) => Some(dialog),
        _ => None,
    }
}

fn dialog_mut(state: &mut AppState) -> Option<&mut QuickMatchDialog> {
    match &mut state.frontend.wol.as_mut()?.page {
        WolPage::QuickMatch(dialog) => Some(dialog),
        _ => None,
    }
}

pub(crate) fn page_open(state: &AppState) -> bool {
    dialog(state).is_some()
}

/// `0x005E2820` on `0x117`'s `0x53F`: `f` gets the page and the adder,
/// which wraps a row to the list's width and appends it (`0x005E29A0`).
fn with_transcript(
    state: &mut AppState,
    f: impl FnOnce(&mut WolQuickMatchPage, &dyn Fn(&mut WolQuickMatchPage, LanChatLine)),
) {
    let layout = layout(state);
    let scale = layout.page.scale;
    let width = scale.base_rect(layout.transcript).w;
    let font = &state.renderer.bit_font;
    let Some(WolPage::QuickMatch(dialog)) =
        state.frontend.wol.as_mut().map(|visit| &mut visit.page)
    else {
        return;
    };
    let add = |ui: &mut WolQuickMatchPage, line: LanChatLine| {
        let visible = ui
            .list
            .geometry(layout.transcript, ui.lines.len(), scale)
            .visible_rows;
        crate::ui::lan_shell::append_list_message(
            &mut ui.lines,
            &mut ui.list,
            line,
            width,
            visible,
            |units| font.text_width(&String::from_utf16_lossy(units)) as i32,
        );
    };
    f(&mut dialog.ui, &add);
}

/// A line for `0x117`'s transcript (`0x005E2700` with `0x117` on top).
pub(crate) fn append_line(state: &mut AppState, line: LanChatLine) {
    with_transcript(state, |ui, add| add(ui, line));
    state.platform.window.request_redraw();
}

pub(crate) fn layout(state: &AppState) -> WolQuickMatchLayout {
    let (width, height) = shell_layout::shell_screen_size(state);
    if shell_layout::fills_window(state) {
        WolQuickMatchLayout::responsive(width as i32, height as i32)
    } else {
        WolQuickMatchLayout::new(width as i32, height as i32)
    }
}

/// The status help under the pointer (`0x006040B0`).
pub(crate) fn status_key(state: &AppState) -> Option<&'static str> {
    dialog(state)?.ui.hovered.and_then(tooltip_key)
}

/// `0x0072BE80`: quickmatch.shp (800 wide only) through quickmatch.PAL.
pub(crate) fn open_art(state: &mut AppState) -> bool {
    if state.frontend.wol_quick_match_art.is_some() {
        return true;
    }
    let Some(assets) = state.process_assets.manager() else {
        return false;
    };
    state.frontend.wol_quick_match_art = crate::render::main_menu_shell_chrome::build_wol_page_art(
        &state.renderer.gpu,
        &state.renderer.batch_renderer,
        assets,
        shell_layout::art_key_screen_width(state),
        ("quickmatch.shp", "quickmatch.PAL"),
        &[],
    );
    state.frontend.wol_quick_match_art.is_some()
}

fn csf(state: &AppState, key: &str) -> String {
    service::resolve(state, key)
}

/// `0x005E2700` into the current dialog's transcript.
fn line(state: &mut AppState, text: String, color: u32) {
    crate::app::shell_lan::append_chat(
        state,
        LanChatLine {
            text: text.encode_utf16().collect(),
            color,
        },
    );
}

fn csf_line(state: &mut AppState, key: &str, color: u32) {
    let text = csf(state, key);
    line(state, text, color);
}

fn csf_name_line(state: &mut AppState, key: &str, name: &str, color: u32) {
    let text = crate::assets::csf_file::format_csf(
        &csf(state, key),
        &[crate::assets::csf_file::CsfArg::Str(name)],
    );
    line(state, text, color);
}

fn nickname(state: &AppState) -> String {
    state.frontend.wol_account.nickname.clone()
}

/// The runner `0x0079FC40`'s entry after the login (`0x0077B439`): the
/// observer preference off, the stage globals, the away text, then the
/// chat-lobby list behind `TXT_REQ_CHANLIST` (`0x0077EF90(0, 0)` and the
/// 30 s wait `0x0077F120`).
/// RESIDUAL: `Network__InitUdpSession` `0x0052F3F0` (the UDP port the
/// request's PRT names) belongs to the transport.
pub(super) fn run(state: &mut AppState) {
    // 0x0079FC40..0x0079FC5D: [0xAC10C8] cleared; 0x5EE350 re-applies it to
    // open dialogs, of which there are none.
    if state.frontend.multiplayer_shell_runtime.lan_observer() {
        state
            .frontend
            .multiplayer_shell_runtime
            .set_lan_observer(false);
    }
    let away = csf(state, "TXT_AWAY_QUICKMATCH").encode_utf16().collect();
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.in_game = false;
    let globals = &mut visit.quick_match;
    globals.failure = 0;
    globals.set(stage::IDLE, None);
    globals.away = Some(away);
    globals.set(stage::JOIN_LOBBY, Some(30));
    globals.opponent = None;
    globals.lobby = None;
    globals.search = None;
    lobby::open_quick_match_runner(state);
}

/// The lobby list arrived (`0x0079FD37..0x0079FE44`): the first lobby whose
/// name starts with the prefix (`_strnicmp`) is the runner's; with no lobby
/// at all `WOL:NoMatchingAgentFound`, with none of the prefix
/// `WOL:MatchingServerUnavailable`. Both boxes end the runner with 0; only
/// the first clears the away text (`0x0079FD66`).
pub(super) fn lobbies_listed(state: &mut AppState, lobbies: &[WolChannel]) {
    let prefix = if quick_coop(state) {
        QUICK_COOP_PREFIX
    } else {
        QUICK_MATCH_PREFIX
    };
    let found = lobbies
        .iter()
        .find(|channel| starts_with(&channel.name, prefix));
    let Some(found) = found.cloned() else {
        let (key, clear_away) = if lobbies.is_empty() {
            ("WOL:NoMatchingAgentFound", true)
        } else {
            ("WOL:MatchingServerUnavailable", false)
        };
        if clear_away && let Some(globals) = globals_mut(state) {
            globals.away = None;
        }
        lobby::quick_match_box(state, key, lobby::QuickMatchBoxThen::Return);
        return;
    };
    if let Some(globals) = globals_mut(state) {
        globals.lobby = Some(found);
    }
    open_dialog(state);
    // 0x0079FEAC..0x0079FF3E: the buddy mirror restarts (API+0xDC; modes
    // 1-3 skip 0x0079CE70), then the ladder queue.
    super::buddies::restart(state);
    advance(state);
}

/// `0x0079FE44..0x0079FE8B`: the art, then `0x117` created, its slide
/// (`0x4A9`) and shown.
fn open_dialog(state: &mut AppState) {
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.page = WolPage::QuickMatch(Box::new(QuickMatchDialog {
        ui: WolQuickMatchPage::default(),
        dot_at: Instant::now() + DOT_PERIOD,
    }));
    state.frontend.shell_route = crate::app::shell_route::ShellRoute::Wol;
    let _ = App::ensure_skirmish_shell_chrome(state);
    open_art(state);
    state.frontend.shell_controller.reset_to(DIALOG, false);
    state.platform.window.request_redraw();
}

/// The runner loop's stage tests (`0x0079FF50..0x0079FFC5`): stage 2 joins
/// the lobby (`0x00788800`; one already joined or pending answers 0) and
/// moves to stage 3 (20 s).
pub(super) fn advance(state: &mut AppState) {
    if !lobby::quick_match_runner(state) || stage(state) != stage::JOIN_LOBBY {
        return;
    }
    let Some(target) = globals(state).and_then(|globals| globals.lobby.clone()) else {
        return;
    };
    lobby::quick_match_join(state, target, QUICK_MATCH_KEY);
    set_stage(state, stage::IN_LOBBY, Some(20));
}

/// The runner's own stage-3 test (`0x0079FFCA..0x007A0009`): with the
/// lobby current, the request, its result ignored. It runs once per event
/// the runner handles, which right after the join is never the lobby's,
/// so in practice only a stage-3 retry (bad `Start` parameters) reaches
/// it.
fn runner_request(state: &mut AppState) {
    let lobby_name = globals(state).and_then(|globals| Some(globals.lobby.as_ref()?.name.clone()));
    if stage(state) == stage::IN_LOBBY
        && lobby::current_channel(state).is_some_and(|current| Some(current.name) == lobby_name)
    {
        request(state);
    }
}

/// The user-list sink in a chat lobby in modes 1-3 (`0x007A7B0D..0x007A7B3B`,
/// no stage test): the request; a refused one sets event 5.
pub(super) fn lobby_users_listed(state: &mut AppState) {
    if !request(state) {
        event_5(state);
    }
}

/// `0x00788800` returned 1 (still in the old channel after its leave):
/// `[0xB77E14] = 1`, stage 0 and `WOL:ChannelJoinFailure`; the runner ends
/// (`0x007A010A..0x007A0149`).
pub(super) fn lobby_join_failed(state: &mut AppState) {
    if let Some(globals) = globals_mut(state) {
        globals.failure = FAILURE_NO_AGENT;
        globals.set(stage::FAILED, None);
    }
    lobby::quick_match_box(
        state,
        "WOL:ChannelJoinFailure",
        lobby::QuickMatchBoxThen::Exit,
    );
}

/// What the request names (`0x007A0872..0x007A0CA2`): the persona's
/// Country, Color and Match By Resolution, the locale table's entry for
/// the nickname, and Options' screen width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MatchRequest {
    pub(crate) country: i32,
    pub(crate) color: i32,
    pub(crate) match_by_resolution: bool,
    /// `None` when the nickname is not in the locale table: no `LOC`.
    pub(crate) locale: Option<i32>,
    pub(crate) width: i32,
}

/// The fields the request takes from the transport and the hardware.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MatchTransport {
    /// `Get_Performance_Profile` `0x005355D0`.
    pub(crate) rating: i32,
    /// `IPXAddressClass::CopyAddressParts` on the local address.
    pub(crate) address: i32,
    /// `[0xABFEC8] & 0xFFFF`.
    pub(crate) nat: i32,
    /// `[0xA8E228]`, the UDP port.
    pub(crate) port: i32,
}

impl MatchRequest {
    /// The text API+0x30 sends, built from the literal pieces at
    /// `0x0084D1F0..0x0084D170` and `sprintf("%d")` numbers.
    pub(crate) fn text(&self, transport: MatchTransport) -> String {
        let mut text = format!(
            "Match COU={}, COL={}, MBR={}",
            self.country,
            self.color,
            u8::from(self.match_by_resolution)
        );
        if let Some(locale) = self.locale {
            text.push_str(&format!(", LOC={locale}"));
        }
        text.push_str(&format!(
            ", RAT={}, RES={}, ADR={}, NAT={}, PRT={}",
            transport.rating, self.width, transport.address, transport.nat, transport.port
        ));
        text
    }
}

/// `0x007A0690`: fewer than two channel users, or no operator, fail
/// (`[0xB77E14] = 1`, stage 0, false); otherwise `WOL:RequestingMatch` and
/// the private request to the matchbot, the last operator in the user
/// list, then stage 4 (180 s).
/// RESIDUAL: the ping lists (`0x007A0430`, `0x007A1060`) and their
/// `Pings` report to the matchbot belong to the transport.
fn request(state: &mut AppState) -> bool {
    if let Some(globals) = globals_mut(state) {
        globals.opponent = None;
    }
    let members = lobby::members(state);
    let matchbot = members
        .iter()
        .rev()
        .find(|member| member.flags & MEMBER_OPERATOR != 0)
        .map(|member| member.name.clone());
    let Some(matchbot) = matchbot.filter(|_| members.len() >= 2) else {
        if let Some(globals) = globals_mut(state) {
            globals.failure = FAILURE_NO_AGENT;
            globals.set(stage::FAILED, None);
        }
        return false;
    };
    // LoadRecord with registry Options bit 1 clear (VERA reads no
    // registry).
    let nickname = nickname(state);
    let persona = super::persona::load(state, &nickname);
    let locale = state
        .frontend
        .wol
        .as_ref()
        .and_then(|visit| visit.locales.get(&nickname.to_ascii_lowercase()))
        .copied();
    let width = lobby::options_screen(state).0;
    csf_line(state, "WOL:RequestingMatch", REQUEST_COLOR);
    service::request(
        state,
        WolServiceRequest::RequestMatch {
            matchbot,
            request: MatchRequest {
                country: persona.country,
                color: persona.color,
                match_by_resolution: persona.match_by_res,
                locale,
                width,
            },
        },
    );
    set_stage(state, stage::REQUESTED, Some(180));
    true
}

/// The ANSI chat sinks in modes 1-3 (`0x007A8DA0`, `0x007A8FC0`): while no
/// match runs, an operator's text goes to `0x007A16B0`; another user's
/// public text is dropped and its private text only logged.
pub(super) fn ansi_message(
    state: &mut AppState,
    from: &crate::ui::wol_lobby::WolMember,
    text: &str,
    private: bool,
    options: Option<Box<crate::app::shell_network_setup::HostOptions>>,
) {
    if from.flags & MEMBER_OPERATOR == 0 {
        if private {
            log::info!("WOL Quick Match private text from {}: {text}", from.name);
        }
        return;
    }
    matchbot_text(state, text, private, options);
    state.platform.window.request_redraw();
}

/// `_strnicmp` of `text` against `prefix`.
fn starts_with(text: &str, prefix: &str) -> bool {
    text.len() >= prefix.len()
        && text.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

/// `0x007A16B0`: the matchbot's commands.
fn matchbot_text(
    state: &mut AppState,
    text: &str,
    private: bool,
    options: Option<Box<crate::app::shell_network_setup::HostOptions>>,
) {
    // `ack` and `nack` (whole, _strcmpi) are ignored; `need ...` is logged.
    if text.eq_ignore_ascii_case("ack") || text.eq_ignore_ascii_case("nack") {
        return;
    }
    let fail = [
        ("Fail", "WOL:MatchFail"),
        ("PingFail", "WOL:MatchFailPing"),
        ("RankFail", "WOL:MatchFailRank"),
        ("WDTSideFail", "WOL:MatchFailWDTSide"),
    ];
    // The longer prefixes first: _strnicmp("PingFail", "Fail") differs.
    for (prefix, key) in fail {
        if private && starts_with(text, prefix) {
            // Event 0x11 re-pings (transport); the stage stays.
            csf_line(state, key, FAIL_COLOR);
            return;
        }
    }
    if starts_with(text, "Exit") {
        csf_line(state, "WOL:MatchFailExit", FAIL_COLOR);
        service::request(state, WolServiceRequest::LeaveChannel);
        lobby::quick_match_box(state, "WOL:MatchFailExit", lobby::QuickMatchBoxThen::Exit);
        return;
    }
    if starts_with(text, "Working") {
        if stage(state) == stage::REQUESTED {
            set_stage(state, stage::WORKING, Some(180));
        }
        csf_line(state, "GUI:MatchingProgress2", u32::MAX);
        return;
    }
    if starts_with(text, "Info ") {
        line(state, text[5..].to_owned(), PROGRESS_COLOR);
        return;
    }
    if starts_with(text, "Pool ") {
        csf_name_line(state, "WOL:MatchingPlayerPool", &text[5..], PROGRESS_COLOR);
        return;
    }
    if private && starts_with(text, "Start ") {
        // strncpy(0xB73714, payload, 0xFF), then event 0x10.
        if let Some(globals) = globals_mut(state) {
            globals.start = options;
        }
        start(state, &text[6..]);
        return;
    }
    if starts_with(text, "need") {
        log::info!("WOL matchbot: {text}");
        return;
    }
    if private {
        csf_line(state, "WOL:MatchUnknownServerCommand", FAIL_COLOR);
    }
}

/// `0x007A1BB0`'s names: `strtok(":")` ends the options, then
/// `strtok(",")` nine times; the first token is the host, the ninth the
/// guest (empty fields merge).
fn start_names(payload: &str) -> (Option<&str>, Option<&str>) {
    let rest = payload.trim_start_matches(':');
    if rest.is_empty() {
        return (None, None);
    }
    let players = rest.find(':').map_or("", |index| &rest[index + 1..]);
    let mut tokens = players.split(',').filter(|token| !token.is_empty());
    let host = tokens.next();
    let guest = tokens.nth(7);
    (host, guest)
}

/// `0x007A1BB0`: the local nickname names the role. Bad names print
/// `WOL:MatchBadParameters` and go back to stage 3 (20 s), which asks
/// again.
/// RESIDUAL: each path's `0x0052F3F0` UDP re-init is the transport's.
fn start(state: &mut AppState, payload: &str) {
    let local = nickname(state);
    let (host, guest) = start_names(payload);
    let is = |name: Option<&str>| {
        name.is_some_and(|name| crate::util::native_string::crt_strcmpi(name, &local).is_eq())
    };
    match (host, guest) {
        (Some(_), Some(guest)) if is(host) => {
            let guest = guest.to_owned();
            found_with(state, &guest);
            csf_line(state, "WOL:MatchCreatingGame", u32::MAX);
            set_stage(state, stage::CREATING, Some(30));
            lobby::quick_match_create(state);
        }
        (Some(host), Some(_)) if is(guest) => {
            let host = host.to_owned();
            found_with(state, &host);
            let name = format!("{host}'s_game");
            csf_name_line(state, "WOL:MatchAttemptingToFindGame", &host, u32::MAX);
            set_stage(state, stage::SEARCHING, Some(30));
            if let Some(globals) = globals_mut(state) {
                globals.search = Some(GameSearch {
                    name,
                    tries: 0,
                    next_at: Some(Instant::now()),
                });
            }
            poll_search(state);
        }
        _ => {
            csf_line(state, "WOL:MatchBadParameters", ERROR_COLOR);
            set_stage(state, stage::IN_LOBBY, Some(20));
            runner_request(state);
        }
    }
}

/// `0x007A19E0`: the opponent, `WOL:MatchFoundWith`, then its ping.
/// RESIDUAL: the ping status (API2+0x20 on the user's address `+0x18`) is
/// the transport's; it is always unknown here.
fn found_with(state: &mut AppState, opponent: &str) {
    if let Some(globals) = globals_mut(state) {
        globals.opponent = Some(opponent.to_owned());
    }
    csf_name_line(state, "WOL:MatchFoundWith", opponent, FOUND_COLOR);
    csf_line(state, "WOL:MatchOpponentPingUnknown", FOUND_COLOR);
}

/// The host's create returned (`0x00789B60`): `WOL:MatchWaitingForJoin`,
/// or `WOL:MatchErrorCreatingGame` and stage 2.
pub(super) fn created(state: &mut AppState, ok: bool) {
    if ok {
        let opponent = globals(state)
            .and_then(|globals| globals.opponent.clone())
            .unwrap_or_default();
        csf_name_line(state, "WOL:MatchWaitingForJoin", &opponent, u32::MAX);
    } else {
        csf_line(state, "WOL:MatchErrorCreatingGame", ERROR_COLOR);
        set_stage(state, stage::JOIN_LOBBY, Some(30));
        advance(state);
    }
}

/// The guest's search: a list request (`0x0077EF90(0x29, 0)`) when its
/// pause is over.
fn poll_search(state: &mut AppState) -> Option<Instant> {
    let search = globals_mut(state)?.search.as_mut()?;
    let at = search.next_at?;
    if Instant::now() < at {
        return Some(at);
    }
    search.next_at = None;
    search.tries += 1;
    service::request(
        state,
        WolServiceRequest::ListChannels {
            kind: GAME_CHANNEL,
            flags: 0,
        },
    );
    None
}

/// The game list arrived during the search: the host's game (`_strcmpi`)
/// is joined (`0x00789960`); missing, the next try follows after 100 ms,
/// and after the last `WOL:MatchGameChannelNotFound` and stage 2 (30 s,
/// `0x007A206E..0x007A20A0`).
/// RESIDUAL: a list request the service refuses, or a 30 s list wait that
/// runs out, prints the same line with stage 3 (`0x007A1F82`); the service
/// reports no list error and the stage-7 timeout ends a lost answer.
pub(super) fn games_listed(state: &mut AppState, games: &[WolChannel]) {
    let Some(search) = globals(state).and_then(|globals| globals.search.clone()) else {
        return;
    };
    if search.next_at.is_some() {
        return;
    }
    let found = games
        .iter()
        .find(|game| crate::util::native_string::crt_strcmpi(&game.name, &search.name).is_eq())
        .cloned();
    if let Some(game) = found {
        if let Some(globals) = globals_mut(state) {
            globals.search = None;
        }
        lobby::quick_match_join_game(state, game);
        return;
    }
    if search.tries < SEARCH_TRIES {
        if let Some(search) = globals_mut(state).and_then(|globals| globals.search.as_mut()) {
            search.next_at = Some(Instant::now() + SEARCH_PAUSE);
        }
        return;
    }
    if let Some(globals) = globals_mut(state) {
        globals.search = None;
    }
    let host = search.name.trim_end_matches("'s_game").to_owned();
    csf_name_line(state, "WOL:MatchGameChannelNotFound", &host, ERROR_COLOR);
    set_stage(state, stage::JOIN_LOBBY, Some(30));
    advance(state);
}

/// The guest's game join answered: `WOL:MatchJoinedGame` and stage 9
/// (90 s), or `WOL:MatchErrorJoiningGame` with the stage left at 7, whose
/// timeout later goes back to stage 2.
pub(super) fn game_joined(state: &mut AppState, ok: bool) {
    if ok {
        csf_line(state, "WOL:MatchJoinedGame", u32::MAX);
        set_stage(state, stage::WAITING, Some(90));
    } else {
        csf_line(state, "WOL:MatchErrorJoiningGame", ERROR_COLOR);
    }
}

/// `0x0077E900`'s Quick Match tail on the host: stage 9 (90 s) as
/// StartGame goes out.
pub(crate) fn start_requested(state: &mut AppState) {
    set_stage(state, stage::WAITING, Some(90));
}

/// OnGameStart (`0x007A7CC3..0x007A7CE9`): `0x117` closes.
pub(crate) fn game_started(state: &mut AppState) {
    close_dialog(state);
}

/// `0x117` closes (`ShellDialog__CloseAndPop`, no slide) and its art is
/// freed (`0x0072BED0`).
fn close_dialog(state: &mut AppState) {
    if let Some(visit) = state.frontend.wol.as_mut()
        && matches!(visit.page, WolPage::QuickMatch(_))
    {
        visit.page = WolPage::Lobby;
    }
    state.frontend.wol_quick_match_art = None;
}

/// Start Game Now's success (event 7, `0x007A029A..0x007A02C6`): the
/// runner returns 1 with the away text still set. Its
/// `WOL:MatchGameStarting` line finds no transcript: `0x117` closed at
/// OnGameStart. The runner's records end with it.
/// `Start_Scenario`'s `0x0078ABF0` (`0x00683D45`) rewrites the away text,
/// so the Quick Match one is gone once the match runs.
/// RESIDUAL: the in-match away text (`0x0052E606`) is not ported.
pub(crate) fn match_starts(state: &mut AppState) {
    if !mode_active(state) {
        return;
    }
    if let Some(globals) = globals_mut(state) {
        globals.away = None;
    }
    close_dialog(state);
    lobby::drop_runner(state);
}

/// The runner returned 0 without its exit tail: its entry list failed
/// (`0x0079FD05`, `0x0079FD1C`, `0x0079FD2E` all clear the away text at
/// `0x0079FD66`).
pub(super) fn runner_closed(state: &mut AppState) {
    if let Some(globals) = globals_mut(state) {
        globals.away = None;
        globals.search = None;
    }
}

/// Start Game Now's own leave of the game channel (API+0x24 at
/// `0x007955F1`): the setup that would carry it ends with the start.
pub(crate) fn start_leave(state: &mut AppState) {
    service::request(state, WolServiceRequest::LeaveChannel);
}

/// The page sinks' auto-reply (`0x007A94ED..0x007A9510`,
/// `0x007A979C..0x007A97C0`): with `[0xB77E40]` set, API+0xEC sends the
/// away text to the pager.
pub(super) fn page_reply(state: &mut AppState, pager: &str) {
    let Some(text) = globals(state).and_then(|globals| globals.away.clone()) else {
        return;
    };
    service::request(
        state,
        WolServiceRequest::Chat {
            line: crate::ui::wol_chat::WolChatLine::Page {
                name: pager.encode_utf16().collect(),
                text,
            },
            to: Vec::new(),
        },
    );
}

/// A runner box closed.
pub(super) fn box_closed(state: &mut AppState, then: lobby::QuickMatchBoxThen) {
    match then {
        lobby::QuickMatchBoxThen::Return => lobby::quick_match_leave_and_return(state),
        lobby::QuickMatchBoxThen::Exit => exit(state),
        lobby::QuickMatchBoxThen::CreateFailed => created(state, false),
    }
}

/// Start Game Now's abort in modes 1-3 (`0x00794D69`, `0x00794E35`): stage
/// 11 and the away text set again; event 6 then ends the runner with 0.
pub(crate) fn start_aborted(state: &mut AppState) {
    let away = csf(state, "TXT_AWAY_QUICKMATCH").encode_utf16().collect();
    if let Some(globals) = globals_mut(state) {
        globals.set(stage::STARTED, None);
        globals.away = Some(away);
    }
    exit(state);
}

/// Event 5 (`0x007A0226..0x007A027D`): only at stage 0 does it end the
/// runner, after the leave and the failure's box.
pub(super) fn event_5(state: &mut AppState) {
    if stage(state) != stage::FAILED {
        return;
    }
    if lobby::in_channel(state) {
        service::request(state, WolServiceRequest::LeaveChannel);
    }
    let key = match globals(state).map_or(0, |globals| globals.failure) {
        FAILURE_NO_AGENT => "WOL:NoMatchingAgentFound",
        FAILURE_TIMEOUT => "WOL:MatchTimeout",
        _ => "WOL:MatchFailExit",
    };
    lobby::quick_match_box(state, key, lobby::QuickMatchBoxThen::Exit);
}

/// The runner's exit (`0x007A0282..0x007A0427`): the away text cleared,
/// `0x117` closed without a slide and its art freed, the stage reset
/// unless Start Game Now ran, then a leave of the current channel behind
/// `WOL:LeavingChannel` (at most 150 x 100 ms) before the runner returns 0.
pub(super) fn exit(state: &mut AppState) {
    if let Some(globals) = globals_mut(state) {
        globals.away = None;
        globals.search = None;
        if globals.stage != stage::STARTED {
            globals.failure = 0;
            globals.set(stage::IDLE, None);
        }
    }
    close_dialog(state);
    // A game channel's nodes go with it.
    if state.frontend.network_setup_protocol.is_some() {
        crate::app::shell_network_setup::abandon_for_wol_runner(state);
    }
    lobby::quick_match_leave_and_return(state);
}

/// `0x117`'s Cancel `0x5C0`: after Start Game Now it only closes the page;
/// a guest's search takes it ([`QuickMatchGlobals::cancel_search`]);
/// otherwise event 0 ends the runner without a box.
fn cancel(state: &mut AppState) {
    if stage(state) == stage::STARTED {
        close_dialog(state);
        return;
    }
    match globals_mut(state).and_then(QuickMatchGlobals::cancel_search) {
        Some(Some(host)) => {
            csf_name_line(state, "WOL:MatchGameChannelNotFound", &host, ERROR_COLOR);
            // 0x007A1BB0 returned: the runner's stage-3 test asks again.
            runner_request(state);
        }
        Some(None) => {}
        None => exit(state),
    }
}

/// The pump `0x0077F900`'s Quick Match half: stage 5 re-arms its 180 s;
/// stages 2-4 fail with `[0xB77E14] = 2` and event 5 (the timeout box);
/// later stages print `WOL:MatchTimeout` and go back to stage 2.
fn pump(state: &mut AppState) {
    let now = now_seconds();
    let Some(globals) = globals(state).filter(|globals| globals.expired(now)) else {
        return;
    };
    match globals.stage {
        stage::WORKING => set_stage(state, stage::WORKING, Some(180)),
        stage::JOIN_LOBBY | stage::IN_LOBBY | stage::REQUESTED => {
            if let Some(globals) = globals_mut(state) {
                globals.failure = FAILURE_TIMEOUT;
                globals.set(stage::FAILED, None);
            }
            event_5(state);
        }
        _ => {
            csf_line(state, "WOL:MatchTimeout", ERROR_COLOR);
            if let Some(globals) = globals_mut(state) {
                globals.failure = 0;
                globals.search = None;
                globals.set(stage::JOIN_LOBBY, Some(30));
            }
            // Under Start Game Now's 0x130 the pump runs from its loops
            // (0x00793D20, 0x007947B0): event 5 waits for the runner, and
            // the start goes on.
            if crate::app::shell_network_setup::verify(state).is_some() {
                return;
            }
            // The guest search ends with its stage; the rejoin's leave
            // takes the game channel and its nodes (0x00785740).
            if state.frontend.network_setup_protocol.is_some() {
                crate::app::shell_network_setup::abandon_for_wol_runner(state);
            }
            advance(state);
        }
    }
}

/// The stage timeouts, the search pauses and `0x117`'s dot timer.
pub(super) fn poll(state: &mut AppState) -> Option<Instant> {
    if !lobby::quick_match_runner(state) {
        return None;
    }
    pump(state);
    let search = poll_search(state);
    let now = Instant::now();
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let mut dot = false;
    let mut redraw = false;
    let dialog_deadlines = dialog_mut(state).map(|dialog| {
        if now >= dialog.dot_at {
            dialog.dot_at = now + DOT_PERIOD;
            dot = true;
        }
        let ui = &mut dialog.ui;
        let geometry = ui
            .list
            .geometry(layout.transcript, ui.lines.len(), layout.page.scale);
        redraw |= ui
            .list
            .scroll
            .poll(geometry, &mut ui.list.top_index, x, y, now);
        [Some(dialog.dot_at), ui.list.scroll.repeat_at()]
    });
    if dot {
        // 0x005E2AC0's rows take colour -1.
        with_transcript(state, |ui, add| {
            ui.dot(|ui, text| {
                add(
                    ui,
                    LanChatLine {
                        text,
                        color: u32::MAX,
                    },
                )
            })
        });
        redraw = true;
    }
    if redraw {
        state.platform.window.request_redraw();
    }
    let stage_deadline = globals(state).and_then(|globals| globals.deadline(now_seconds()));
    [stage_deadline, search]
        .into_iter()
        .chain(dialog_deadlines.into_iter().flatten())
        .flatten()
        .min()
}

/// Diagnostics: the current stage's timeout has passed.
pub(crate) fn expire_stage(state: &mut AppState) {
    if let Some(globals) = globals_mut(state)
        && let Some(timeout) = globals.timeout
    {
        globals.stamp = globals.stamp.saturating_sub(timeout + 1);
    }
}

pub(super) fn cursor_moved(state: &mut AppState) {
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let Some(dialog) = dialog_mut(state) else {
        return;
    };
    let ui = &mut dialog.ui;
    ui.hovered = layout.control_at(x, y);
    ui.buttons.hovered = ui.hovered.filter(|id| *id == CANCEL);
    let geometry = ui
        .list
        .geometry(layout.transcript, ui.lines.len(), layout.page.scale);
    ui.list
        .scroll
        .pointer_moved(geometry, &mut ui.list.top_index, x, y);
    state.frontend.shell_status_line.repaint();
    state.platform.window.request_redraw();
}

pub(super) fn mouse(state: &mut AppState, pressed: bool) {
    if shell_transition::blocks_shell_input(state) {
        return;
    }
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let over = layout.control_at(x, y);
    let Some(dialog) = dialog_mut(state) else {
        return;
    };
    let ui = &mut dialog.ui;
    if !pressed {
        ui.list.scroll.cancel();
        if ui.buttons.release(over.filter(|id| *id == CANCEL)) == Some(CANCEL) {
            cancel(state);
        }
        state.platform.window.request_redraw();
        return;
    }
    let geometry = ui
        .list
        .geometry(layout.transcript, ui.lines.len(), layout.page.scale);
    let mut sound = false;
    if let Some(part) = geometry.scroll_part_at(x, y) {
        ui.list
            .scroll
            .press(part, geometry, &mut ui.list.top_index, y, Instant::now());
    } else if over == Some(CANCEL) {
        ui.buttons.press(Some(CANCEL));
        sound = true;
    } else {
        ui.buttons.press(None);
    }
    if sound {
        // Owner-draw buttons play GUIMainButtonSound on the press.
        App::play_main_menu_button_sound(state);
    }
    state.platform.window.request_redraw();
}

/// RunMain's postmatch re-entry in modes 1/2 (`0x0077B439..0x0077B442`):
/// `0x007A2970`, then mode 0, whose category change logs out before the
/// welcome. `0x007A2970` clears `[0xB77DC1]` and asks `0x122` only without a
/// lost connection (`[0xB77DC2]`), with `[Ask] AddMatchBuddies` and an
/// opponent that may become a buddy (`0x0079C520`).
pub(super) fn postmatch(state: &mut AppState) {
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.in_game = false;
    let disconnected = visit.disconnected;
    let opponent = visit.quick_match.opponent.clone().unwrap_or_default();
    let ask = super::persona::info_file(state).is_some_and(|file| file.ask_add_match_buddies());
    let local = nickname(state);
    if !disconnected && ask && state.frontend.wol_buddies.valid_new(&opponent, &local) {
        super::persona::open_add_buddy_query(state, &opponent);
        return;
    }
    welcome(state);
}

/// Mode 0 after `0x007A2970`: the network visit ends and the welcome runs.
fn welcome(state: &mut AppState) {
    state.match_state.session.leave_network();
    App::reopen_wol_welcome(state);
}

/// `0x122`'s teardown has run (`0x007A29E0`): both buttons write
/// `[Ask] AddMatchBuddies` from Don't Ask; Yes adds the opponent while it
/// may still become a buddy (API+0xE0, the mirror, the locate and ladder
/// queues). Then mode 0 and the welcome.
pub(crate) fn add_buddy_answered(state: &mut AppState, yes: bool, ask_again: bool) {
    if let Some(file) = super::persona::info_file(state) {
        file.set_ask_add_match_buddies(ask_again);
    }
    let opponent = globals(state)
        .and_then(|globals| globals.opponent.clone())
        .unwrap_or_default();
    let local = nickname(state);
    if yes && state.frontend.wol_buddies.valid_new(&opponent, &local) {
        super::buddies::add(state, opponent);
    }
    welcome(state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_names_its_fields_and_omits_an_unknown_locale() {
        let request = MatchRequest {
            country: -2,
            color: 3,
            match_by_resolution: true,
            locale: Some(2),
            width: 800,
        };
        let transport = MatchTransport {
            rating: 5,
            address: -1062731519,
            nat: 0,
            port: 1234,
        };
        assert_eq!(
            request.text(transport),
            "Match COU=-2, COL=3, MBR=1, LOC=2, RAT=5, RES=800, ADR=-1062731519, NAT=0, PRT=1234"
        );
        let request = MatchRequest {
            locale: None,
            match_by_resolution: false,
            ..request
        };
        assert_eq!(
            request.text(MatchTransport::default()),
            "Match COU=-2, COL=3, MBR=0, RAT=0, RES=800, ADR=0, NAT=0, PRT=0"
        );
    }

    #[test]
    fn start_names_take_the_first_and_ninth_player_tokens() {
        let payload = "0,1,2:Alpha,1,2,,3,4,5,6,7,Beta,9";
        assert_eq!(start_names(payload), (Some("Alpha"), Some("Beta")));
        assert_eq!(start_names("::x:A,1,2"), (Some("A"), None));
        assert_eq!(start_names("no players"), (None, None));
        assert_eq!(start_names(""), (None, None));
    }

    #[test]
    fn cancel_ends_only_a_search_whose_list_request_is_out() {
        let mut globals = QuickMatchGlobals::default();
        assert_eq!(globals.cancel_search(), None);
        globals.set(stage::SEARCHING, Some(30));
        globals.search = Some(GameSearch {
            name: "Alpha's_game".into(),
            tries: 2,
            next_at: Some(Instant::now()),
        });
        assert_eq!(globals.cancel_search(), Some(None));
        assert_eq!(globals.stage, stage::SEARCHING);
        globals.search.as_mut().unwrap().next_at = None;
        assert_eq!(globals.cancel_search(), Some(Some("Alpha".into())));
        assert!(globals.search.is_none());
        assert_eq!(
            (globals.stage, globals.timeout),
            (stage::IN_LOBBY, Some(20))
        );
    }

    #[test]
    fn stage_timeouts_are_strict_and_skip_the_resting_stages() {
        let mut globals = QuickMatchGlobals::default();
        globals.set(stage::REQUESTED, Some(180));
        let stamp = globals.stamp;
        assert!(!globals.expired(stamp + 180));
        assert!(globals.expired(stamp + 181));
        for resting in [stage::FAILED, stage::IDLE, stage::STARTED] {
            globals.stage = resting;
            assert!(!globals.expired(stamp + 1000));
        }
        globals.set(stage::JOIN_LOBBY, None);
        assert!(!globals.expired(globals.stamp + 1000));
    }
}
