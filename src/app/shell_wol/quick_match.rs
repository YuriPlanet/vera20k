//! The Quick Match runner `0x0079FC40` (WOL mode 1) and its page `0x117`
//! (proc `0x007A2110`), from the login to a match or back to the welcome
//! (for VERA's online lobby, back to that lobby), and the postmatch
//! add-buddy query `0x122` (`0x007A2970`).
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
//! ([`crate::app::shell_network_setup`]). Quick Co-op (mode 2) runs the
//! same runner; [`mode_active`] names where the two differ.

use crate::util::time::{Duration, Instant};

use super::WolPage;
use super::leaf::{self, Pointer};
use super::lobby;
use super::service::{self, WolServiceRequest};
use crate::app::frontend::state::FrontendState;
use crate::app::frontend::{shell_layout, shell_transition};
use crate::app::process_assets::ProcessAssets;
use crate::app::shell_wol::WolVisit;
use crate::app::{App, AppState};
use crate::ui::lan_shell::LanChatLine;
use crate::ui::shell::page::{self, PageEvent};
use crate::ui::wol_lobby::{GAME_CHANNEL, MEMBER_OPERATOR, WolChannel};
use crate::ui::wol_login::WolAccountGlobals;
use crate::ui::wol_quick_match::*;
use crate::ui::wol_shell::WolMode;
use vera20k_gui::{NodeId, Prop, Value};

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
    /// The status help of the control last hovered on the page itself: a
    /// box over it takes the pointer and leaves the help as it was.
    help: Option<String>,
}

/// `time()`.
fn now_seconds() -> u64 {
    crate::util::time::SystemTime::now()
        .duration_since(crate::util::time::UNIX_EPOCH)
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
pub(crate) fn mode_active(wol: Option<&WolVisit>) -> bool {
    wol.is_some_and(|visit| matches!(visit.mode, Some(WolMode::QuickMatch | WolMode::QuickCoop)))
}

/// `[0xA8B244] == 2`: Quick Co-op.
pub(crate) fn quick_coop(wol: Option<&WolVisit>) -> bool {
    wol.is_some_and(|visit| visit.mode == Some(WolMode::QuickCoop))
}

fn globals(wol: Option<&WolVisit>) -> Option<&QuickMatchGlobals> {
    Some(&wol?.quick_match)
}

fn globals_mut(wol: Option<&mut WolVisit>) -> Option<&mut QuickMatchGlobals> {
    Some(&mut wol?.quick_match)
}

/// The runner's stage (`[0xB73A20]`).
pub(crate) fn stage(wol: Option<&WolVisit>) -> u8 {
    globals(wol).map_or(stage::IDLE, |globals| globals.stage)
}

fn set_stage(wol: Option<&mut WolVisit>, stage: u8, timeout: Option<u64>) {
    if let Some(globals) = globals_mut(wol) {
        globals.set(stage, timeout);
    }
}

/// The matchbot's options for the host (`[0xB73714]`).
pub(crate) fn start_options(
    wol: Option<&WolVisit>,
) -> Option<crate::app::shell_network_setup::HostOptions> {
    globals(wol)?.start.as_deref().cloned()
}

pub(crate) fn dialog(wol: Option<&WolVisit>) -> Option<&QuickMatchDialog> {
    match &wol?.page {
        WolPage::QuickMatch(dialog) => Some(dialog),
        _ => None,
    }
}

fn dialog_mut(wol: Option<&mut WolVisit>) -> Option<&mut QuickMatchDialog> {
    match &mut wol?.page {
        WolPage::QuickMatch(dialog) => Some(dialog),
        _ => None,
    }
}

pub(crate) fn page_open(wol: Option<&WolVisit>) -> bool {
    dialog(wol).is_some()
}

/// `0x005E2820` on `0x117`'s `0x53F`: `f` gets the page and the adder,
/// which wraps a row to the list's width and appends it (`0x005E29A0`);
/// the list widget then keeps its view ([`vera20k_gui::Gui::log_rows`]).
fn with_transcript(
    frontend: &mut FrontendState,
    renderer: &RendererState,
    f: impl FnOnce(&mut WolQuickMatchPage, &dyn Fn(&mut WolQuickMatchPage, LanChatLine)),
) {
    let layout = layout(&renderer.gpu);
    let rect = layout
        .rect_of(crate::ui::wol_quick_match::TRANSCRIPT)
        .unwrap_or_default();
    let width = layout.page.scale.base_rect(rect).w;
    let root = sync(frontend, renderer);
    let gui = frontend.shell_controller.gui_mut();
    let font = &renderer.bit_font;
    let Some(WolPage::QuickMatch(dialog)) = frontend.wol.as_mut().map(|visit| &mut visit.page)
    else {
        return;
    };
    let dropped = std::cell::Cell::new(0);
    let add = |ui: &mut WolQuickMatchPage, line: LanChatLine| {
        let measure = |units: &[u16]| font.text_width(&String::from_utf16_lossy(units)) as i32;
        let gone = crate::ui::lan_shell::append_wrapped(&mut ui.lines, line, width, measure);
        dropped.set(dropped.get() + gone);
    };
    f(&mut dialog.ui, &add);
    if let Some(list) = root.and_then(|root| gui.find_id(root, TRANSCRIPT.into())) {
        // A list built after its rows came starts at its first row.
        if gui.get(list, Prop::Top).ok().flatten().is_none() {
            let _ = gui.set(list, Prop::Top, Value::Int(0));
        }
        let _ = gui.log_rows(list, dialog.ui.lines.len(), dropped.get());
    }
}

/// A line for `0x117`'s transcript (`0x005E2700` with `0x117` on top).
pub(crate) fn append_line(
    frontend: &mut FrontendState,
    renderer: &RendererState,
    line: LanChatLine,
) {
    with_transcript(frontend, renderer, |ui, add| add(ui, line));
}

pub(crate) fn layout(gpu: &GpuContext) -> WolQuickMatchLayout {
    shell_layout::on(gpu, WolQuickMatchLayout::new)
}

/// The status help under the pointer (`0x006040B0`).
pub(crate) fn status_key(wol: Option<&WolVisit>) -> Option<&str> {
    dialog(wol)?.help.as_deref()
}

/// Builds and places `0x117`'s widgets; under a box they take no pointer.
pub(crate) fn sync(frontend: &mut FrontendState, renderer: &RendererState) -> Option<NodeId> {
    let layout = layout(&renderer.gpu);
    let boxed = App::wol_box_owns_input(frontend.wol.as_ref());
    let root = leaf::sync(
        &mut frontend.shell_controller,
        &renderer.bit_font,
        DIALOG,
        layout.page.scale,
        spec,
        |id| layout.rect_of(id),
    )?;
    let gui = frontend.shell_controller.gui_mut();
    for id in [CANCEL, TRANSCRIPT] {
        let hit = layout.rect_of(id).filter(|_| !boxed).unwrap_or_default();
        page::set(gui, root, id, Prop::HitRect, Value::Rect(hit));
    }
    Some(root)
}

/// `0x005E2700` into the current dialog's transcript.
fn line(
    frontend: &mut FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    text: String,
    color: u32,
) {
    crate::app::shell_lan::append_chat(
        renderer,
        frontend,
        match_state,
        process_assets,
        LanChatLine {
            text: text.encode_utf16().collect(),
            color,
        },
    );
}

fn csf_line(
    frontend: &mut FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    key: &str,
    color: u32,
) {
    let text = process_assets.csf_text(key);
    line(frontend, match_state, process_assets, renderer, text, color);
}

fn csf_name_line(
    frontend: &mut FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    key: &str,
    name: &str,
    color: u32,
) {
    let text = crate::assets::csf_file::format_csf(
        &process_assets.csf_text(key),
        &[crate::assets::csf_file::CsfArg::Str(name)],
    );
    line(frontend, match_state, process_assets, renderer, text, color);
}

fn nickname(account: &WolAccountGlobals) -> String {
    account.nickname.clone()
}

/// The runner `0x0079FC40`'s entry after the login (`0x0077B439`): the
/// observer preference off, the stage globals, the away text, then the
/// chat-lobby list behind `TXT_REQ_CHANLIST` (`0x0077EF90(0, 0)` and the
/// 30 s wait `0x0077F120`).
/// RESIDUAL: `Network__InitUdpSession` `0x0052F3F0` (the UDP port the
/// request's PRT names) belongs to the transport.
pub(super) fn run(
    frontend: &mut FrontendState,
    session: &mut MatchSession,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    options_profile: &RetailOptionsProfile,
) {
    // 0x0079FC40..0x0079FC5D: [0xAC10C8] cleared; 0x5EE350 re-applies it to
    // open dialogs, of which there are none.
    if frontend.multiplayer_shell_runtime.lan_observer() {
        frontend.multiplayer_shell_runtime.set_lan_observer(false);
    }
    let away = process_assets
        .csf_text("TXT_AWAY_QUICKMATCH")
        .encode_utf16()
        .collect();
    let Some(visit) = frontend.wol.as_mut() else {
        return;
    };
    let globals = &mut visit.quick_match;
    globals.failure = 0;
    globals.set(stage::IDLE, None);
    globals.away = Some(away);
    globals.set(stage::JOIN_LOBBY, Some(30));
    globals.opponent = None;
    globals.lobby = None;
    globals.search = None;
    lobby::open_runner(
        frontend,
        session,
        process_assets,
        renderer,
        options_profile,
        lobby::Runner::QuickMatch,
    );
}

/// The refusal that names the changed game files (`{files}`).
const CHANGED_FILES: &str = "VERA:QuickMatchChangedFiles";

use crate::app::match_runtime::session::MatchSession;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::renderer_state::RendererState;
use crate::app::shell_main_menu::ClientStep;
use crate::app::shell_network_setup::agreement::{
    OriginalGameChange, changed_files_text, original_game_changes,
};
use crate::audio::sfx::SfxPlayer;
use crate::render::gpu::GpuContext;
use crate::util::config::GameConfig;
use winit::window::Window;

/// VERA: Quick Match pairs players who agreed on nothing, so it plays the
/// original game. A mod fails the pre-match check. Rules, Art, AI or mode
/// INIs that differ, by root or changed file, fail the join's rules hash
/// (`0x0079BA60`, silently in Quick Match) against any opponent without the
/// same; every other change (another compared INI root, the theater suffix,
/// other changed game files, the Ares setting On) fails the start's rules
/// digest. The Ares setting Off is no change: without a mod it opens the
/// game Auto opens. The box says what to turn off for the first change
/// ([`original_game_changes`]), and the runner ends before it queues.
fn original_game_refusal(
    mod_title: Option<&str>,
    assets: &crate::app::process_assets::ProcessAssets,
) -> Option<&'static str> {
    Some(match original_game_changes(mod_title, assets).first()? {
        OriginalGameChange::Mod(_) => "VERA:QuickMatchNoMod",
        #[cfg(target_os = "android")]
        OriginalGameChange::Ra2 => "VERA:QuickMatchRa2ModeAndroid",
        #[cfg(not(target_os = "android"))]
        OriginalGameChange::Ra2 => "VERA:QuickMatchRa2Mode",
        OriginalGameChange::IniRoots => "VERA:QuickMatchIniProfile",
        OriginalGameChange::Ares(_) => "VERA:QuickMatchAresSetting",
        OriginalGameChange::Kratos => "VERA:QuickMatchKratos",
        OriginalGameChange::ChangedFiles(_) => CHANGED_FILES,
    })
}

/// The lobby list arrived (`0x0079FD37..0x0079FE44`): the first lobby whose
/// name starts with the prefix (`_strnicmp`) is the runner's; with no lobby
/// at all `WOL:NoMatchingAgentFound`, with none of the prefix
/// `WOL:MatchingServerUnavailable`. Both boxes end the runner with 0; only
/// the first clears the away text (`0x0079FD66`).
pub(super) fn lobbies_listed(state: &mut AppState) {
    if let Some(key) = original_game_refusal(state.frontend.mods.title(), &state.process_assets) {
        lobby::quick_match_box(
            &mut state.frontend,
            &state.process_assets,
            key,
            lobby::QuickMatchBoxThen::Return,
        );
        if key == CHANGED_FILES {
            let files = changed_files_text(state.process_assets.changed_game_files());
            lobby::replace_box_body(
                state.frontend.wol.as_mut(),
                &crate::i18n::tr_fill(key, &[("files", &files)]),
            );
        }
        return;
    }
    let prefix = if quick_coop(state.frontend.wol.as_ref()) {
        QUICK_COOP_PREFIX
    } else {
        QUICK_MATCH_PREFIX
    };
    let lobbies = lobby::lobbies(state.frontend.wol.as_ref());
    let found = lobbies
        .iter()
        .find(|channel| starts_with(&channel.name, prefix))
        .cloned();
    let Some(found) = found else {
        let (key, clear_away) = if lobbies.is_empty() {
            ("WOL:NoMatchingAgentFound", true)
        } else {
            ("WOL:MatchingServerUnavailable", false)
        };
        if clear_away && let Some(globals) = globals_mut(state.frontend.wol.as_mut()) {
            globals.away = None;
        }
        lobby::quick_match_box(
            &mut state.frontend,
            &state.process_assets,
            key,
            lobby::QuickMatchBoxThen::Return,
        );
        return;
    };
    if let Some(globals) = globals_mut(state.frontend.wol.as_mut()) {
        globals.lobby = Some(found);
    }
    open_dialog(
        &mut state.frontend,
        &state.process_assets,
        &state.renderer,
        &state.persistence.options_profile,
    );
    // 0x0079FEAC..0x0079FF3E: the buddy mirror restarts (API+0xDC; modes
    // 1-3 skip 0x0079CE70), then the ladder queue.
    super::buddies::restart(&mut state.frontend);
    advance(state);
}

/// `0x0079FE44..0x0079FE8B`: the art, then `0x117` created, its slide
/// (`0x4A9`) and shown.
fn open_dialog(
    frontend: &mut FrontendState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    options_profile: &RetailOptionsProfile,
) {
    let Some(visit) = frontend.wol.as_mut() else {
        return;
    };
    visit.page = WolPage::QuickMatch(Box::new(QuickMatchDialog {
        ui: WolQuickMatchPage::default(),
        dot_at: Instant::now() + DOT_PERIOD,
        help: None,
    }));
    frontend.shell_route = crate::app::shell_route::ShellRoute::Wol;
    let _ = App::ensure_skirmish_shell_chrome(renderer, frontend, process_assets, options_profile);
    super::leaf::open_art(frontend, process_assets, renderer, DIALOG);
    frontend.shell_controller.reset_to(DIALOG);
}

/// The runner loop's stage tests (`0x0079FF50..0x0079FFC5`): stage 2 joins
/// the lobby (`0x00788800`; one already joined or pending answers 0) and
/// moves to stage 3 (20 s), then falls into the stage-3 test. Every way
/// back to stage 2 returns to that loop: a guest's failed search and a
/// failed create (`0x007A1BB0` under event 0x10), a timeout (the pump's
/// event 5, `0x0077F900`).
pub(super) fn advance(state: &mut AppState) {
    if !lobby::quick_match_runner(state.frontend.wol.as_ref())
        || stage(state.frontend.wol.as_ref()) != stage::JOIN_LOBBY
    {
        return;
    }
    let Some(target) =
        globals(state.frontend.wol.as_ref()).and_then(|globals| globals.lobby.clone())
    else {
        return;
    };
    let step = lobby::quick_match_join(
        &mut state.frontend,
        &state.platform.window,
        &mut state.match_state,
        &state.process_assets,
        &state.renderer,
        &state.persistence.options_profile,
        target,
        QUICK_MATCH_KEY,
    );
    if let Some(step) = step {
        App::apply_step(state, step);
    }
    set_stage(state.frontend.wol.as_mut(), stage::IN_LOBBY, Some(20));
    runner_request(
        &mut state.frontend,
        state.platform.game_config.as_ref(),
        &state.match_state,
        &state.process_assets,
        &state.renderer,
        &state.persistence.options_profile,
    );
}

/// The runner's own stage-3 test (`0x0079FFCA..0x007A0009`): with the
/// lobby current, the request, its result ignored. Right after a join the
/// lobby is not current yet and its user list asks instead
/// ([`lobby_users_listed`]); a runner that never left the lobby asks here:
/// a guest whose search found no game, as when its host crashed after the
/// pairing, or a bad `Start`. Native never runs it during a leave
/// ([`lobby::leaving`]): a timeout there finds the lobby already left.
fn runner_request(
    frontend: &mut FrontendState,
    game_config: Option<&GameConfig>,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    options_profile: &RetailOptionsProfile,
) {
    let lobby_name = globals(frontend.wol.as_ref())
        .and_then(|globals| Some(globals.lobby.as_ref()?.name.clone()));
    if stage(frontend.wol.as_ref()) == stage::IN_LOBBY
        && !lobby::leaving(frontend.wol.as_ref())
        && lobby::current_channel(frontend.wol.as_ref())
            .is_some_and(|current| Some(current.name) == lobby_name)
    {
        request(
            frontend,
            game_config,
            match_state,
            process_assets,
            renderer,
            options_profile,
        );
    }
}

/// The user-list sink in a chat lobby in modes 1-3 (`0x007A7B0D..0x007A7B3B`,
/// no stage test): the request; a refused one sets event 5.
pub(super) fn lobby_users_listed(
    frontend: &mut FrontendState,
    game_config: Option<&GameConfig>,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    options_profile: &RetailOptionsProfile,
) {
    if !request(
        frontend,
        game_config,
        match_state,
        process_assets,
        renderer,
        options_profile,
    ) {
        event_5(frontend, process_assets);
    }
}

/// `0x00788800` returned 1 (still in the old channel after its leave):
/// `[0xB77E14] = 1`, stage 0 and `WOL:ChannelJoinFailure`; the runner ends
/// (`0x007A010A..0x007A0149`).
pub(super) fn lobby_join_failed(frontend: &mut FrontendState, process_assets: &ProcessAssets) {
    if let Some(globals) = globals_mut(frontend.wol.as_mut()) {
        globals.failure = FAILURE_NO_AGENT;
        globals.set(stage::FAILED, None);
    }
    lobby::quick_match_box(
        frontend,
        process_assets,
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
fn request(
    frontend: &mut FrontendState,
    game_config: Option<&GameConfig>,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    options_profile: &RetailOptionsProfile,
) -> bool {
    if let Some(globals) = globals_mut(frontend.wol.as_mut()) {
        globals.opponent = None;
    }
    let members = lobby::members(frontend.wol.as_ref());
    let matchbot = members
        .iter()
        .rev()
        .find(|member| member.flags & MEMBER_OPERATOR != 0)
        .filter(|_| members.len() >= 2)
        .map(|member| member.name.clone());
    let Some(matchbot) = matchbot else {
        if let Some(globals) = globals_mut(frontend.wol.as_mut()) {
            globals.failure = FAILURE_NO_AGENT;
            globals.set(stage::FAILED, None);
        }
        return false;
    };
    // LoadRecord with registry Options bit 1 clear (VERA reads no
    // registry).
    let nickname = nickname(&frontend.wol_account);
    let persona = super::persona::load(
        frontend.wol.as_ref(),
        game_config,
        process_assets,
        &nickname,
    );
    let locale = frontend
        .wol
        .as_ref()
        .and_then(|visit| visit.locales.get(&nickname.to_ascii_lowercase()))
        .copied();
    let width = lobby::options_screen(&renderer.gpu, options_profile).0;
    // An online visit has no persona file: the MultiPlayer side and colour
    // the player saved in VERA.INI stay (`persona::take_side_and_color`),
    // and they go instead, as the game's first `R` sends them
    // (`send_first_choices`).
    let (country, color) = match super::persona::info_file(frontend.wol.as_ref(), game_config) {
        Some(_) => (persona.country, persona.color),
        None => crate::app::frontend::skirmish_session::seat_items(
            &frontend.multiplayer_shell_runtime.local_participant().slot,
        ),
    };
    // Quick Match plays in YR's eight colours: one of VERA's extra lobby
    // colours goes as Random, which the paired games draw from the eight.
    let color = if (0..crate::skirmish_launch::HOUSE_COLOR_COUNT as i32).contains(&color) {
        color
    } else {
        crate::app::frontend::skirmish_session::RANDOM_ITEM_DATA
    };
    csf_line(
        frontend,
        match_state,
        process_assets,
        renderer,
        "WOL:RequestingMatch",
        REQUEST_COLOR,
    );
    service::request(
        frontend.wol.as_mut(),
        WolServiceRequest::RequestMatch {
            matchbot,
            request: MatchRequest {
                country,
                color,
                match_by_resolution: persona.match_by_res,
                locale,
                width,
            },
        },
    );
    set_stage(frontend.wol.as_mut(), stage::REQUESTED, Some(180));
    true
}

/// The ANSI chat sinks in modes 1-3 (`0x007A8DA0`, `0x007A8FC0`): while no
/// match runs, an operator's text goes to `0x007A16B0`; another user's
/// public text is dropped and its private text only logged.
pub(super) fn ansi_message(
    frontend: &mut FrontendState,
    game_config: Option<&GameConfig>,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    options_profile: &RetailOptionsProfile,
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
    matchbot_text(
        frontend,
        game_config,
        match_state,
        process_assets,
        renderer,
        options_profile,
        text,
        private,
        options,
    );
}

/// `_strnicmp` of `text` against `prefix`.
fn starts_with(text: &str, prefix: &str) -> bool {
    text.len() >= prefix.len()
        && text.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

/// `0x007A16B0`: the matchbot's commands.
fn matchbot_text(
    frontend: &mut FrontendState,
    game_config: Option<&GameConfig>,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    options_profile: &RetailOptionsProfile,
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
            csf_line(
                frontend,
                match_state,
                process_assets,
                renderer,
                key,
                FAIL_COLOR,
            );
            return;
        }
    }
    if starts_with(text, "Exit") {
        csf_line(
            frontend,
            match_state,
            process_assets,
            renderer,
            "WOL:MatchFailExit",
            FAIL_COLOR,
        );
        service::request(frontend.wol.as_mut(), WolServiceRequest::LeaveChannel);
        lobby::quick_match_box(
            frontend,
            process_assets,
            "WOL:MatchFailExit",
            lobby::QuickMatchBoxThen::Exit,
        );
        return;
    }
    if starts_with(text, "Working") {
        if stage(frontend.wol.as_ref()) == stage::REQUESTED {
            set_stage(frontend.wol.as_mut(), stage::WORKING, Some(180));
        }
        csf_line(
            frontend,
            match_state,
            process_assets,
            renderer,
            "GUI:MatchingProgress2",
            u32::MAX,
        );
        return;
    }
    if starts_with(text, "Info ") {
        line(
            frontend,
            match_state,
            process_assets,
            renderer,
            text[5..].to_owned(),
            PROGRESS_COLOR,
        );
        return;
    }
    if starts_with(text, "Pool ") {
        csf_name_line(
            frontend,
            match_state,
            process_assets,
            renderer,
            "WOL:MatchingPlayerPool",
            &text[5..],
            PROGRESS_COLOR,
        );
        return;
    }
    if private && starts_with(text, "Start ") {
        // strncpy(0xB73714, payload, 0xFF), then event 0x10.
        if let Some(globals) = globals_mut(frontend.wol.as_mut()) {
            globals.start = options;
        }
        start(
            frontend,
            game_config,
            match_state,
            process_assets,
            renderer,
            options_profile,
            &text[6..],
        );
        return;
    }
    // Five bytes, the space included (`0x007A1987`): a bare "need" or
    // "needs" is an unknown command.
    if starts_with(text, "need ") {
        log::info!("WOL matchbot: {text}");
        return;
    }
    if private {
        csf_line(
            frontend,
            match_state,
            process_assets,
            renderer,
            "WOL:MatchUnknownServerCommand",
            FAIL_COLOR,
        );
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

/// The local player's part in a `Start` (`0x007A1BB0`).
#[derive(Debug, PartialEq, Eq)]
enum StartRole<'a> {
    Host { guest: &'a str },
    Guest { host: &'a str },
}

/// `0x007A1BB0`'s test: the local nickname (`_strcmpi`) is the first
/// player, the host, or else the ninth, the guest; `None` for bad names.
fn start_role<'a>(payload: &'a str, local: &str) -> Option<StartRole<'a>> {
    let (host, guest) = start_names(payload);
    let is = |name: Option<&str>| {
        name.is_some_and(|name| crate::util::native_string::crt_strcmpi(name, local).is_eq())
    };
    match (host, guest) {
        (Some(_), Some(guest)) if is(host) => Some(StartRole::Host { guest }),
        (Some(host), Some(_)) if is(guest) => Some(StartRole::Guest { host }),
        _ => None,
    }
}

/// `0x007A1BB0`: the local nickname names the role. Bad names print
/// `WOL:MatchBadParameters` and go back to stage 3 (20 s), which asks
/// again.
/// RESIDUAL: each path's `0x0052F3F0` UDP re-init is the transport's.
fn start(
    frontend: &mut FrontendState,
    game_config: Option<&GameConfig>,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    options_profile: &RetailOptionsProfile,
    payload: &str,
) {
    let local = nickname(&frontend.wol_account);
    match start_role(payload, &local) {
        Some(StartRole::Host { guest }) => {
            let guest = guest.to_owned();
            found_with(frontend, match_state, process_assets, renderer, &guest);
            csf_line(
                frontend,
                match_state,
                process_assets,
                renderer,
                "WOL:MatchCreatingGame",
                u32::MAX,
            );
            set_stage(frontend.wol.as_mut(), stage::CREATING, Some(30));
            lobby::quick_match_create(
                frontend,
                game_config,
                &match_state.session,
                process_assets,
                &renderer.gpu,
                options_profile,
            );
        }
        Some(StartRole::Guest { host }) => {
            let host = host.to_owned();
            found_with(frontend, match_state, process_assets, renderer, &host);
            let name = format!("{host}'s_game");
            csf_name_line(
                frontend,
                match_state,
                process_assets,
                renderer,
                "WOL:MatchAttemptingToFindGame",
                &host,
                u32::MAX,
            );
            set_stage(frontend.wol.as_mut(), stage::SEARCHING, Some(30));
            if let Some(globals) = globals_mut(frontend.wol.as_mut()) {
                globals.search = Some(GameSearch {
                    name,
                    tries: 0,
                    next_at: Some(Instant::now()),
                });
            }
            poll_search(frontend.wol.as_mut());
        }
        None => {
            csf_line(
                frontend,
                match_state,
                process_assets,
                renderer,
                "WOL:MatchBadParameters",
                ERROR_COLOR,
            );
            set_stage(frontend.wol.as_mut(), stage::IN_LOBBY, Some(20));
            runner_request(
                frontend,
                game_config,
                match_state,
                process_assets,
                renderer,
                options_profile,
            );
        }
    }
}

/// One player of a matchbot's pairing: the name and the request's `COU`
/// and `COL`, My Information's item data (-2 Random).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub(crate) struct MatchPlayer {
    pub(crate) name: String,
    pub(crate) country: i32,
    pub(crate) color: i32,
}

/// The pairing VERA20K Online's matchbot sends beside its private `Start`
/// (`quick_match`, docs/lobby-protocol.md): a fresh seed, the host (the
/// earlier waiter) and the guest.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub(crate) struct MatchStart {
    pub(crate) seed: u32,
    pub(crate) host: MatchPlayer,
    pub(crate) guest: MatchPlayer,
}

/// The scenario of a pairing's options.
#[derive(Debug, Clone, Copy)]
pub(crate) enum MatchMap<'a> {
    /// A named scenario (the stand-in matchbot's Quick Co-op).
    Named(&'a str),
    /// The first scenario the mode admits for two (the stand-in's Quick
    /// Match).
    First,
    /// VERA20K Online's: [`official_duel`].
    OfficialDuel,
}

/// The pairing this game hosts: a private `Start` whose host
/// [`start_role`] finds to be the local player. The guest builds nothing.
pub(super) fn hosted_pairing(
    local: &str,
    text: &str,
    private: bool,
    pairing: MatchStart,
) -> Option<MatchStart> {
    let host = private
        && starts_with(text, "Start ")
        && matches!(start_role(&text[6..], local), Some(StartRole::Host { .. }));
    host.then_some(pairing)
}

/// The host's options for the VERA20K Online pairing it hosts
/// ([`hosted_pairing`]), which the `Start` message carries (`[0xB73714]`);
/// `None` for a pairing that cannot be built (logged).
pub(super) fn online_start_options(
    frontend: &FrontendState,
    game_config: Option<&GameConfig>,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    pairing: MatchStart,
) -> Option<Box<crate::app::shell_network_setup::HostOptions>> {
    match host_options(
        frontend,
        game_config,
        match_state,
        process_assets,
        &pairing,
        MatchMap::OfficialDuel,
    ) {
        Ok(options) => Some(Box::new(options)),
        Err(error) => {
            log::warn!("WOL Quick Match: no host options for the matchbot's pairing: {error:#}");
            None
        }
    }
}

/// The host's options for a matchbot's pairing, as `0x005E3D10` parses a
/// `Start` payload into them for the host (`0x007A6DC2..0x007A6E50`): the
/// first mode (Cooperative in Quick Co-op), the host on row 0 and the
/// guest on row 1 with their countries and colours on automatic starts
/// (Random ones drawn by [`draw_random_picks`]), and the pairing's seed. The test service's stand-in matchbot and VERA20K
/// Online's both build them here.
pub(crate) fn host_options(
    frontend: &FrontendState,
    game_config: Option<&GameConfig>,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    pairing: &MatchStart,
    map: MatchMap<'_>,
) -> anyhow::Result<crate::app::shell_network_setup::HostOptions> {
    use crate::ui::skirmish_shell::{SkirmishComboItem, SkirmishCountryChoice};
    let modes = &frontend.skirmish_modes;
    let mode = if quick_coop(frontend.wol.as_ref()) {
        modes
            .iter()
            .find(|mode| mode.class == crate::skirmish_modes::MpModeClass::Cooperative)
    } else {
        modes.first()
    }
    .ok_or_else(|| anyhow::anyhow!("no mode"))?;
    // My Information's countries (0x4E3A00).
    let countries: Vec<_> = crate::app::state::active_rules(frontend, match_state)
        .map(crate::ui::skirmish_shell::country_choices)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| match item {
            SkirmishComboItem::Country(SkirmishCountryChoice::Country(country)) => Some(country),
            _ => None,
        })
        .collect();
    build_host_options(
        mode,
        frontend.scenario_catalog.records(),
        &countries,
        pairing,
        map,
        frontend.skirmish_shell_state.launch_options_base.clone(),
        |file| {
            crate::app::shell_network_setup::local_map_metadata(
                game_config,
                &frontend.scenario_catalog,
                process_assets,
                file,
            )
        },
    )
}

/// [`host_options`] over the host's mode, catalog and countries.
fn build_host_options(
    mode: &crate::skirmish_modes::SkirmishGameMode,
    records: &[crate::map::skirmish_scenarios::SkirmishScenarioRecord],
    countries: &[crate::skirmish_launch::LaunchCountry],
    pairing: &MatchStart,
    map: MatchMap<'_>,
    options: crate::skirmish_launch::SkirmishLaunchOptions,
    map_metadata: impl FnOnce(
        &str,
    )
        -> anyhow::Result<crate::app::match_runtime::session::NetworkMapMetadata>,
) -> anyhow::Result<crate::app::shell_network_setup::HostOptions> {
    use crate::app::frontend::skirmish_session::{RANDOM_ITEM_DATA, persona_seat};
    use crate::app::match_runtime::session::{NetworkAiOptions, NetworkVacancy, PeerChoicePolicy};
    use crate::skirmish_launch::{NETWORK_PLAYER_SLOT_COUNT, SkirmishHumanSlot, SkirmishLocalSlot};
    let record = match map {
        MatchMap::Named(file) => records
            .iter()
            .find(|record| record.file_name.eq_ignore_ascii_case(file)),
        MatchMap::First => records.iter().find(|record| {
            crate::map::skirmish_scenarios::record_matches_mode(record, mode)
                && record.player_capacity >= 2
        }),
        MatchMap::OfficialDuel => official_duel(records, mode, pairing.seed),
    };
    let file = record
        .map(|record| record.file_name.clone())
        .ok_or_else(|| anyhow::anyhow!("no map for the matchbot"))?;
    // Only what My Information can store: a country it lists, a colour, or
    // Random.
    let seat = |player: &MatchPlayer| -> anyhow::Result<SkirmishLocalSlot> {
        let country_item = crate::app::frontend::skirmish_shell_render::side_item_data_for_country;
        anyhow::ensure!(
            player.country == RANDOM_ITEM_DATA
                || countries
                    .iter()
                    .any(|country| country_item(*country) == player.country),
            "{} has no country {}",
            player.name,
            player.country
        );
        anyhow::ensure!(
            player.color == RANDOM_ITEM_DATA
                || (0..crate::skirmish_launch::HOUSE_COLOR_COUNT as i32).contains(&player.color),
            "{} has no colour {}",
            player.name,
            player.color
        );
        Ok(persona_seat(player.country, player.color))
    };
    let (host_seat, guest_seat) = (seat(&pairing.host)?, seat(&pairing.guest)?);
    let human = |row: u8, name: &str, slot| SkirmishHumanSlot {
        raw_slot_index: Some(row),
        name: name.into(),
        slot,
        role: Default::default(),
    };
    let mut launch = crate::skirmish_launch::SkirmishLaunchSession::from_rows(
        crate::skirmish_launch::SkirmishLaunchMode::from_game_mode(mode),
        Some(file.clone()),
        vec![
            human(0, &pairing.host.name, host_seat),
            human(
                1,
                &pairing.guest.name,
                persona_seat(RANDOM_ITEM_DATA, RANDOM_ITEM_DATA),
            ),
        ],
        0,
        Vec::new(),
        options,
    );
    // 0x0069B600: the guest's picks join as a peer's do, so a colour the
    // host holds turns Random.
    crate::app::match_runtime::session::apply_peer_choices(
        &mut launch,
        &[NetworkAiOptions::INITIAL_INACTIVE; NETWORK_PLAYER_SLOT_COUNT],
        1,
        guest_seat,
        false,
        PeerChoicePolicy::Wol {
            countries: None,
            teams: None,
        },
        crate::skirmish_launch::HOUSE_COLOR_COUNT,
    );
    draw_random_picks(&mut launch.humans, countries, pairing.seed)?;
    Ok(crate::app::shell_network_setup::HostOptions {
        launch,
        map_metadata: map_metadata(&file)?,
        vacancies: [NetworkVacancy::Open; NETWORK_PLAYER_SLOT_COUNT],
        observer_row: None,
        cooperative_difficulty: 0,
        seed: Some(pairing.seed),
    })
}

/// The pairing's Random picks, host first: a country of My Information's
/// list and a colour the other player does not hold, drawn from the seed.
/// Both players then accept with a concrete side and colour
/// (`0x007A7F72`).
/// RESIDUAL: the retail matchbot sent concrete countries and colours in its
/// `Start`; VERA's lobby holds no rules data and echoes each request's
/// `COU`/`COL`, Random included, so the host draws them. The draws are
/// VERA's, not native's.
fn draw_random_picks(
    humans: &mut [crate::skirmish_launch::SkirmishHumanSlot],
    countries: &[crate::skirmish_launch::LaunchCountry],
    seed: u32,
) -> anyhow::Result<()> {
    anyhow::ensure!(!countries.is_empty(), "no country to draw");
    let mut draws = (0u32..).map(|k| seat_draw(seed, k) as usize);
    for index in 0..humans.len() {
        let held: Vec<u8> = humans
            .iter()
            .enumerate()
            .filter(|(other, human)| *other != index && !human.slot.color_random)
            .map(|(_, human)| human.slot.color_index)
            .collect();
        let slot = &mut humans[index].slot;
        if slot.country_random {
            slot.country = countries[draws.next().expect("endless") % countries.len()];
            slot.country_random = false;
        }
        if slot.color_random {
            let free: Vec<u8> = (0..crate::skirmish_launch::HOUSE_COLOR_COUNT as u8)
                .filter(|color| !held.contains(color))
                .collect();
            slot.color_index = free[draws.next().expect("endless") % free.len()];
            slot.color_random = false;
        }
    }
    Ok(())
}

/// The seed's `k`th draw for a Random pick: a Weyl step, then a 32-bit
/// integer hash, so the picks do not follow the map's `seed % count`.
fn seat_draw(seed: u32, k: u32) -> u32 {
    let mut x = seed.wrapping_add(k.wrapping_add(1).wrapping_mul(0x9E37_79B9));
    x = (x ^ (x >> 16)).wrapping_mul(0x7FEB_352D);
    x = (x ^ (x >> 15)).wrapping_mul(0x846C_A68B);
    x ^ (x >> 16)
}

/// VERA20K Online's map: of the official two-player scenarios the mode
/// admits, in catalog order, the seed's (`seed % count`). Only maps with a
/// file here count, not the random map's row or a map only in the online
/// library.
/// RESIDUAL: the retail matchbot named the scenario in its `Start` options
/// (`0x005E3D10` parses it); VERA's lobby holds no retail map data, so the
/// host picks. Every online Quick Match plays VERA's pool and pick, not
/// the retail ladder's.
fn official_duel<'a>(
    records: &'a [crate::map::skirmish_scenarios::SkirmishScenarioRecord],
    mode: &crate::skirmish_modes::SkirmishGameMode,
    seed: u32,
) -> Option<&'a crate::map::skirmish_scenarios::SkirmishScenarioRecord> {
    let pool = || {
        records.iter().filter(|record| {
            record.official
                && record.player_capacity == 2
                && record.kind == crate::map::skirmish_scenarios::SkirmishScenarioKind::ConcreteMap
                && !record.is_online()
                && crate::map::skirmish_scenarios::record_matches_mode(record, mode)
        })
    };
    let count = pool().count();
    pool().nth((seed as usize).checked_rem(count)?)
}

/// `0x007A19E0`: the opponent, `WOL:MatchFoundWith`, then its ping.
/// RESIDUAL: the ping status (API2+0x20 on the user's address `+0x18`) is
/// the transport's; it is always unknown here.
fn found_with(
    frontend: &mut FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    opponent: &str,
) {
    if let Some(globals) = globals_mut(frontend.wol.as_mut()) {
        globals.opponent = Some(opponent.to_owned());
    }
    csf_name_line(
        frontend,
        match_state,
        process_assets,
        renderer,
        "WOL:MatchFoundWith",
        opponent,
        FOUND_COLOR,
    );
    csf_line(
        frontend,
        match_state,
        process_assets,
        renderer,
        "WOL:MatchOpponentPingUnknown",
        FOUND_COLOR,
    );
}

/// The host's create returned (`0x00789B60`): `WOL:MatchWaitingForJoin`,
/// or `WOL:MatchErrorCreatingGame` and stage 2.
pub(super) fn created(state: &mut AppState, ok: bool) {
    if ok {
        let opponent = globals(state.frontend.wol.as_ref())
            .and_then(|globals| globals.opponent.clone())
            .unwrap_or_default();
        csf_name_line(
            &mut state.frontend,
            &state.match_state,
            &state.process_assets,
            &state.renderer,
            "WOL:MatchWaitingForJoin",
            &opponent,
            u32::MAX,
        );
    } else {
        csf_line(
            &mut state.frontend,
            &state.match_state,
            &state.process_assets,
            &state.renderer,
            "WOL:MatchErrorCreatingGame",
            ERROR_COLOR,
        );
        set_stage(state.frontend.wol.as_mut(), stage::JOIN_LOBBY, Some(30));
        advance(state);
    }
}

/// The guest's search: a list request (`0x0077EF90(0x29, 0)`) when its
/// pause is over.
fn poll_search(mut wol: Option<&mut WolVisit>) -> Option<Instant> {
    let search = globals_mut(wol.as_deref_mut())?.search.as_mut()?;
    let at = search.next_at?;
    if Instant::now() < at {
        return Some(at);
    }
    search.next_at = None;
    search.tries += 1;
    service::request(
        wol.as_deref_mut(),
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
/// reports no list error and the stage-7 timeout ends a lost answer. The
/// found game's join step is returned.
#[must_use]
pub(super) fn games_listed(state: &mut AppState, games: &[WolChannel]) -> Option<ClientStep> {
    let search = globals(state.frontend.wol.as_ref()).and_then(|globals| globals.search.clone())?;
    if search.next_at.is_some() {
        return None;
    }
    let found = games
        .iter()
        .find(|game| crate::util::native_string::crt_strcmpi(&game.name, &search.name).is_eq())
        .cloned();
    if let Some(game) = found {
        if let Some(globals) = globals_mut(state.frontend.wol.as_mut()) {
            globals.search = None;
        }
        return lobby::quick_match_join_game(
            &mut state.frontend,
            &state.platform.window,
            &mut state.match_state,
            &state.process_assets,
            &state.renderer,
            &state.persistence.options_profile,
            game,
        );
    }
    if search.tries < SEARCH_TRIES {
        if let Some(search) =
            globals_mut(state.frontend.wol.as_mut()).and_then(|globals| globals.search.as_mut())
        {
            search.next_at = Some(Instant::now() + SEARCH_PAUSE);
        }
        return None;
    }
    if let Some(globals) = globals_mut(state.frontend.wol.as_mut()) {
        globals.search = None;
    }
    let host = search.name.trim_end_matches("'s_game").to_owned();
    csf_name_line(
        &mut state.frontend,
        &state.match_state,
        &state.process_assets,
        &state.renderer,
        "WOL:MatchGameChannelNotFound",
        &host,
        ERROR_COLOR,
    );
    set_stage(state.frontend.wol.as_mut(), stage::JOIN_LOBBY, Some(30));
    advance(state);
    None
}

/// The guest's game join answered: `WOL:MatchJoinedGame` and stage 9
/// (90 s), or `WOL:MatchErrorJoiningGame` with the stage left at 7, whose
/// timeout later goes back to stage 2.
pub(super) fn game_joined(
    frontend: &mut FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    ok: bool,
) {
    if ok {
        csf_line(
            frontend,
            match_state,
            process_assets,
            renderer,
            "WOL:MatchJoinedGame",
            u32::MAX,
        );
        set_stage(frontend.wol.as_mut(), stage::WAITING, Some(90));
    } else {
        csf_line(
            frontend,
            match_state,
            process_assets,
            renderer,
            "WOL:MatchErrorJoiningGame",
            ERROR_COLOR,
        );
    }
}

/// The admission `0x0079BA60` refused the host's game, so `0x00789960`
/// sends no join and the join answers false ([`game_joined`]).
/// VERA: gamemd's Quick Match says no more; the refusal's line follows.
/// A rules difference is worded as the pre-match check words it:
/// `TXT_MISMATCH` sends players to Westwood's site for a newer version.
pub(super) fn game_refused(
    frontend: &mut FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    reason: Option<&'static str>,
) {
    game_joined(frontend, match_state, process_assets, renderer, false);
    if let Some(key) = refusal_line(reason) {
        csf_line(
            frontend,
            match_state,
            process_assets,
            renderer,
            key,
            ERROR_COLOR,
        );
    }
}

fn refusal_line(reason: Option<&'static str>) -> Option<&'static str> {
    match reason? {
        "TXT_MISMATCH" => Some("VERA:MatchStartMismatch"),
        key => Some(key),
    }
}

/// `0x0077E900`'s Quick Match tail on the host: stage 9 (90 s) as
/// StartGame goes out.
pub(crate) fn start_requested(wol: Option<&mut WolVisit>) {
    set_stage(wol, stage::WAITING, Some(90));
}

/// OnGameStart (`0x007A7CC3..0x007A7CE9`): `0x117` closes.
pub(crate) fn game_started(frontend: &mut FrontendState) {
    close_dialog(frontend);
}

/// `0x117` closes (`ShellDialog__CloseAndPop`, no slide) and its art is
/// freed (`0x0072BED0`).
fn close_dialog(frontend: &mut FrontendState) {
    if let Some(visit) = frontend.wol.as_mut()
        && matches!(visit.page, WolPage::QuickMatch(_))
    {
        visit.page = WolPage::Lobby;
    }
    super::leaf::free_art(&mut frontend.wol_art, DIALOG);
}

/// Start Game Now's success (event 7, `0x007A029A..0x007A02C6`): the
/// runner returns 1 with the away text still set. Its
/// `WOL:MatchGameStarting` line finds no transcript: `0x117` closed at
/// OnGameStart. The runner's records end with it.
/// `Start_Scenario`'s `0x0078ABF0` (`0x00683D45`) rewrites the away text,
/// so the Quick Match one is gone once the match runs.
/// RESIDUAL: the in-match away text (`0x0052E606`) is not ported.
pub(crate) fn match_starts(frontend: &mut FrontendState) {
    if !mode_active(frontend.wol.as_ref()) {
        return;
    }
    if let Some(globals) = globals_mut(frontend.wol.as_mut()) {
        globals.away = None;
    }
    close_dialog(frontend);
    lobby::drop_runner(frontend.wol.as_mut());
}

/// The runner returned 0 without its exit tail: its entry list failed
/// (`0x0079FD05`, `0x0079FD1C`, `0x0079FD2E` all clear the away text at
/// `0x0079FD66`).
pub(super) fn runner_closed(wol: Option<&mut WolVisit>) {
    if let Some(globals) = globals_mut(wol) {
        globals.away = None;
        globals.search = None;
    }
}

/// Start Game Now's own leave of the game channel (API+0x24 at
/// `0x007955F1`): the setup that would carry it ends with the start.
pub(crate) fn start_leave(wol: Option<&mut WolVisit>) {
    service::request(wol, WolServiceRequest::LeaveChannel);
}

/// The page sinks' auto-reply (`0x007A94ED..0x007A9510`,
/// `0x007A979C..0x007A97C0`): with `[0xB77E40]` set, API+0xEC sends the
/// away text to the pager.
pub(super) fn page_reply(mut wol: Option<&mut WolVisit>, pager: &str) {
    let Some(text) = globals(wol.as_deref()).and_then(|globals| globals.away.clone()) else {
        return;
    };
    service::request(
        wol.as_deref_mut(),
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
    let away = state
        .process_assets
        .csf_text("TXT_AWAY_QUICKMATCH")
        .encode_utf16()
        .collect();
    if let Some(globals) = globals_mut(state.frontend.wol.as_mut()) {
        globals.set(stage::STARTED, None);
        globals.away = Some(away);
    }
    exit(state);
}

/// Event 5 (`0x007A0226..0x007A027D`): only at stage 0 does it end the
/// runner, after the leave and the failure's box.
pub(super) fn event_5(frontend: &mut FrontendState, process_assets: &ProcessAssets) {
    if stage(frontend.wol.as_ref()) != stage::FAILED {
        return;
    }
    if lobby::in_channel(frontend.wol.as_ref()) {
        service::request(frontend.wol.as_mut(), WolServiceRequest::LeaveChannel);
    }
    let key = match globals(frontend.wol.as_ref()).map_or(0, |globals| globals.failure) {
        FAILURE_NO_AGENT => "WOL:NoMatchingAgentFound",
        FAILURE_TIMEOUT => "WOL:MatchTimeout",
        _ => "WOL:MatchFailExit",
    };
    lobby::quick_match_box(
        frontend,
        process_assets,
        key,
        lobby::QuickMatchBoxThen::Exit,
    );
}

/// The runner's exit (`0x007A0282..0x007A0427`): the away text cleared,
/// `0x117` closed without a slide and its art freed, the stage reset
/// unless Start Game Now ran, then a leave of the current channel behind
/// `WOL:LeavingChannel` (at most 150 x 100 ms) before the runner returns 0.
pub(super) fn exit(state: &mut AppState) {
    if let Some(globals) = globals_mut(state.frontend.wol.as_mut()) {
        globals.away = None;
        globals.search = None;
        if globals.stage != stage::STARTED {
            globals.failure = 0;
            globals.set(stage::IDLE, None);
        }
    }
    close_dialog(&mut state.frontend);
    // A game channel's nodes go with it.
    if state.frontend.network_setup_protocol.is_some() {
        crate::app::shell_network_setup::abandon_for_wol_runner(
            &mut state.frontend,
            &mut state.match_state,
        );
    }
    lobby::quick_match_leave_and_return(state);
}

/// `0x117`'s Cancel `0x5C0`: after Start Game Now it only closes the page;
/// a guest's search takes it ([`QuickMatchGlobals::cancel_search`]);
/// otherwise event 0 ends the runner without a box, the returned step.
#[must_use]
fn cancel(
    frontend: &mut FrontendState,
    game_config: Option<&GameConfig>,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    options_profile: &RetailOptionsProfile,
) -> Option<ClientStep> {
    if stage(frontend.wol.as_ref()) == stage::STARTED {
        close_dialog(frontend);
        return None;
    }
    match globals_mut(frontend.wol.as_mut()).and_then(QuickMatchGlobals::cancel_search) {
        Some(Some(host)) => {
            csf_name_line(
                frontend,
                match_state,
                process_assets,
                renderer,
                "WOL:MatchGameChannelNotFound",
                &host,
                ERROR_COLOR,
            );
            // 0x007A1BB0 returned: the runner's stage-3 test asks again.
            runner_request(
                frontend,
                game_config,
                match_state,
                process_assets,
                renderer,
                options_profile,
            );
            None
        }
        Some(None) => None,
        None => Some(ClientStep::Now(Box::new(exit))),
    }
}

/// The pump `0x0077F900`'s Quick Match half: stage 5 re-arms its 180 s;
/// stages 2-4 fail with `[0xB77E14] = 2` and event 5 (the timeout box);
/// later stages print `WOL:MatchTimeout` and go back to stage 2.
fn pump(state: &mut AppState) {
    let now = now_seconds();
    let Some(globals) = globals(state.frontend.wol.as_ref()).filter(|globals| globals.expired(now))
    else {
        return;
    };
    match globals.stage {
        stage::WORKING => set_stage(state.frontend.wol.as_mut(), stage::WORKING, Some(180)),
        stage::JOIN_LOBBY | stage::IN_LOBBY | stage::REQUESTED => {
            if let Some(globals) = globals_mut(state.frontend.wol.as_mut()) {
                globals.failure = FAILURE_TIMEOUT;
                globals.set(stage::FAILED, None);
            }
            event_5(&mut state.frontend, &state.process_assets);
        }
        _ => {
            csf_line(
                &mut state.frontend,
                &state.match_state,
                &state.process_assets,
                &state.renderer,
                "WOL:MatchTimeout",
                ERROR_COLOR,
            );
            if let Some(globals) = globals_mut(state.frontend.wol.as_mut()) {
                globals.failure = 0;
                globals.search = None;
                globals.set(stage::JOIN_LOBBY, Some(30));
            }
            // Under Start Game Now's 0x130 the pump runs from its loops
            // (0x00793D20, 0x007947B0): event 5 waits for the runner, and
            // the start goes on.
            if crate::app::shell_network_setup::verify(
                state.frontend.network_setup_protocol.as_ref(),
            )
            .is_some()
            {
                return;
            }
            // The guest search ends with its stage; the rejoin's leave
            // takes the game channel and its nodes (0x00785740).
            if state.frontend.network_setup_protocol.is_some() {
                crate::app::shell_network_setup::abandon_for_wol_runner(
                    &mut state.frontend,
                    &mut state.match_state,
                );
            }
            advance(state);
        }
    }
}

/// The stage timeouts, the search pauses and `0x117`'s dot timer.
pub(super) fn poll(state: &mut AppState) -> Option<Instant> {
    if !lobby::quick_match_runner(state.frontend.wol.as_ref()) {
        return None;
    }
    pump(state);
    let search = poll_search(state.frontend.wol.as_mut());
    let now = Instant::now();
    let mut dot = false;
    let dot_at = dialog_mut(state.frontend.wol.as_mut()).map(|dialog| {
        if now >= dialog.dot_at {
            dialog.dot_at = now + DOT_PERIOD;
            dot = true;
        }
        dialog.dot_at
    });
    let repeat = leaf::poll(
        &mut state.frontend.shell_controller,
        &state.platform.window,
        dot_at.is_some(),
    );
    if dot {
        // 0x005E2AC0's rows take colour -1.
        with_transcript(&mut state.frontend, &state.renderer, |ui, add| {
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
    }
    let stage_deadline =
        globals(state.frontend.wol.as_ref()).and_then(|globals| globals.deadline(now_seconds()));
    [stage_deadline, search, dot_at, repeat]
        .into_iter()
        .flatten()
        .min()
}

/// Diagnostics: the current stage's timeout has passed.
pub(crate) fn expire_stage(wol: Option<&mut WolVisit>) {
    if let Some(globals) = globals_mut(wol)
        && let Some(timeout) = globals.timeout
    {
        globals.stamp = globals.stamp.saturating_sub(timeout + 1);
    }
}

pub(super) fn cursor_moved(
    frontend: &mut FrontendState,
    window: &Window,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    sfx_player: &mut Option<SfxPlayer>,
) {
    sync(frontend, renderer);
    leaf::pointer(
        frontend,
        window,
        match_state,
        process_assets,
        sfx_player,
        Pointer::Move,
    );
    let help = leaf::status_key(&frontend.shell_controller, DIALOG).map(str::to_owned);
    if let Some(dialog) = dialog_mut(frontend.wol.as_mut()) {
        dialog.help = help;
    }
}

/// `0x117`'s buttons; Cancel's runner exit is the returned step.
#[must_use]
pub(super) fn mouse(
    frontend: &mut FrontendState,
    window: &Window,
    game_config: Option<&GameConfig>,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    options_profile: &RetailOptionsProfile,
    sfx_player: &mut Option<SfxPlayer>,
    pressed: bool,
) -> Option<ClientStep> {
    if shell_transition::blocks_shell_input(frontend) || sync(frontend, renderer).is_none() {
        return None;
    }
    let events = leaf::pointer(
        frontend,
        window,
        match_state,
        process_assets,
        sfx_player,
        if pressed { Pointer::Down } else { Pointer::Up },
    );
    if events.contains(&PageEvent::Activated(CANCEL)) {
        return cancel(
            frontend,
            game_config,
            match_state,
            process_assets,
            renderer,
            options_profile,
        );
    }
    None
}

/// RunMain's postmatch re-entry in modes 1/2 (`0x0077B439..0x0077B442`):
/// `0x007A2970`, then mode 0, whose category change logs out before the
/// welcome (VERA: the online lobby, [`return_to_lobby`]). `0x007A2970` clears `[0xB77DC1]` and asks `0x122` only without a
/// lost connection (`[0xB77DC2]`), with `[Ask] AddMatchBuddies` and an
/// opponent that may become a buddy (`0x0079C520`).
pub(super) fn postmatch(state: &mut AppState) {
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.in_game = false;
    let disconnected = visit.disconnected;
    let opponent = visit.quick_match.opponent.clone().unwrap_or_default();
    let ask = super::persona::info_file(
        state.frontend.wol.as_ref(),
        state.platform.game_config.as_ref(),
    )
    .is_some_and(|file| file.ask_add_match_buddies());
    let local = nickname(&state.frontend.wol_account);
    if !disconnected && ask && state.frontend.wol_buddies.valid_new(&opponent, &local) {
        super::persona::open_add_buddy_query(
            &mut state.frontend,
            &state.process_assets,
            &state.renderer,
            &state.persistence.options_profile,
            &opponent,
        );
        return;
    }
    welcome(state);
}

/// Mode 0 after `0x007A2970`: the network visit ends and the welcome runs,
/// or the online lobby that started the runner opens again.
fn welcome(state: &mut AppState) {
    state.match_state.session.leave_network();
    if return_to_lobby(state) {
        return;
    }
    App::reopen_wol_welcome(state);
}

/// VERA: the runner ended (`0x007A0282`, or after `0x007A2970`) and the
/// online lobby started it. Native modes 1 and 2 always end in mode 0
/// (`WOL__RunMain` reads the return mode `[0xA8B248]` only for mode 5),
/// which in the online visit would sign the player out. This takes return
/// mode 4 as after Buddy List instead, rejoining the lobby whatever its
/// population. Returns false for mode 0.
pub(super) fn return_to_lobby(state: &mut AppState) -> bool {
    let Some(visit) = state
        .frontend
        .wol
        .as_mut()
        .filter(|visit| visit.return_mode == Some(WolMode::CustomMatch))
    else {
        return false;
    };
    visit.return_mode = None;
    visit.lobby_joined_before = false;
    visit.lobby = None;
    super::reopen_lobby(state);
    true
}

/// `0x122`'s teardown has run (`0x007A29E0`): both buttons write
/// `[Ask] AddMatchBuddies` from Don't Ask; Yes adds the opponent while it
/// may still become a buddy (API+0xE0, the mirror, the locate and ladder
/// queues). Then mode 0 and the welcome, or the online lobby: the returned
/// step.
pub(crate) fn add_buddy_answered(
    frontend: &mut FrontendState,
    game_config: Option<&GameConfig>,
    yes: bool,
    ask_again: bool,
) -> ClientStep {
    if let Some(file) = super::persona::info_file(frontend.wol.as_ref(), game_config) {
        file.set_ask_add_match_buddies(ask_again);
    }
    let opponent = globals(frontend.wol.as_ref())
        .and_then(|globals| globals.opponent.clone())
        .unwrap_or_default();
    let local = nickname(&frontend.wol_account);
    if yes && frontend.wol_buddies.valid_new(&opponent, &local) {
        super::buddies::add(frontend, opponent);
    }
    ClientStep::Now(Box::new(welcome))
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    #[test]
    fn the_changed_files_box_names_three_and_counts_the_rest() {
        let files = [
            "rulesmd.ini",
            "clear01.sno",
            "clear02.sno",
            "clear03.sno",
            "clear04.sno",
        ]
        .map(String::from);
        assert_eq!(
            changed_files_text(&files[..3]),
            "rulesmd.ini, clear01.sno, clear02.sno"
        );
        assert_eq!(
            changed_files_text(&files),
            "rulesmd.ini, clear01.sno, clear02.sno (+2)"
        );
    }

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

    /// Official two-player maps among a four-player map, an unofficial
    /// duel and the random map's two-player row.
    fn duel_records() -> Vec<crate::map::skirmish_scenarios::SkirmishScenarioRecord> {
        use crate::map::skirmish_scenarios::*;
        let record = |file: &str, capacity, official| SkirmishScenarioRecord {
            source_ordinal: 0,
            source: SkirmishScenarioSource::MissionsMdPkt,
            file_name: file.into(),
            display_name: file.into(),
            title_label: None,
            author: None,
            preview: Default::default(),
            multiplayer_start_waypoints: Vec::new(),
            player_capacity: capacity,
            preview_source_bounds: None,
            game_modes: vec!["standard".into()],
            min_players: None,
            max_players: None,
            official,
            kind: SkirmishScenarioKind::ConcreteMap,
        };
        vec![
            record("four.map", 4, true),
            record("custom.yrm", 2, false),
            record("duel0.map", 2, true),
            SkirmishScenarioRecord::random_map_sentinel(3, "Random", 2),
            record("duel1.map", 2, true),
        ]
    }

    #[test]
    fn the_online_map_is_the_seeds_official_duel() {
        let records = duel_records();
        let battle = &crate::skirmish_modes::stock_skirmish_modes()[0];
        let pick = |seed| official_duel(&records, battle, seed).map(|r| r.file_name.as_str());
        assert_eq!(pick(4), Some("duel0.map"));
        assert_eq!(pick(4), pick(4));
        assert_eq!(pick(u32::MAX), Some("duel1.map"));
        assert_eq!(official_duel(&records[..2], battle, 4), None);
    }

    #[test]
    fn a_pairing_seats_both_players_for_its_host_and_nothing_for_its_guest() {
        let pairing: MatchStart = serde_json::from_value(serde_json::json!({
            "seed": 7,
            "host": {"name": "Alpha", "country": -2, "color": 3},
            "guest": {"name": "Beta", "country": 5, "color": 3},
        }))
        .unwrap();
        let text = "Start 7:Alpha,0,0,0,0,0,0,0,Beta,0,0,0,0,0,0,0";
        assert_eq!(hosted_pairing("Beta", text, true, pairing.clone()), None);
        assert_eq!(hosted_pairing("Alpha", text, false, pairing.clone()), None);
        let pairing = hosted_pairing("alpha", text, true, pairing).unwrap();

        let battle = &crate::skirmish_modes::stock_skirmish_modes()[0];
        let countries: Vec<_> = (0..10)
            .map(crate::skirmish_launch::LaunchCountry::from_country_index)
            .collect();
        let build = |pairing: &MatchStart| {
            build_host_options(
                battle,
                &duel_records(),
                &countries,
                pairing,
                MatchMap::OfficialDuel,
                Default::default(),
                |file| {
                    Ok(crate::app::match_runtime::session::NetworkMapMetadata {
                        display_name: file.into(),
                        player_capacity: 2,
                        official: true,
                        byte_length: 0,
                        digest: String::new(),
                        source_sha256: String::new(),
                        effective_digest: String::new(),
                    })
                },
            )
        };
        let options = build(&pairing).unwrap();
        let launch = &options.launch;
        let seats: Vec<_> = launch
            .humans
            .iter()
            .map(|human| (human.name.as_str(), human.raw_slot_index))
            .collect();
        assert_eq!(seats, [("Alpha", Some(0)), ("Beta", Some(1))]);
        assert_eq!(launch.local_player_index, 0);
        assert_eq!(launch.selected_map_file.as_deref(), Some("duel1.map"));
        assert_eq!(options.seed, Some(7));
        // Random draws a listed country; the colour the host holds turns
        // Random and draws another, so both players accept.
        let (host, guest) = (&launch.humans[0].slot, &launch.humans[1].slot);
        assert!(!host.country_random && countries.contains(&host.country));
        assert_eq!((host.color_random, host.color_index), (false, 3));
        assert!(!guest.country_random && guest.country == countries[5]);
        assert!(!guest.color_random && guest.color_index != 3);
        assert_eq!(build(&pairing).unwrap().launch, options.launch);

        let mut foreign = pairing;
        foreign.guest.country = 70_000;
        assert!(build(&foreign).is_err());
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
    fn changes_to_the_original_end_quick_match_before_it_queues() {
        use crate::app::process_assets::ProcessAssets;
        use crate::assets::asset_manager::MediaArchiveMode;
        use crate::rules::personal_rules::PersonalRules;
        use crate::util::ares_launch::AresSetting;
        // The Options switches stay home: Quick Match's room shares none,
        // so its match plays without them.
        let mut changes = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false)
            .with_personal_rules(PersonalRules::default());
        changes.set_terrain_deformation(true);
        assert_eq!(original_game_refusal(None, &changes), None);
        // A loose rulesmd.ini the retail archives don't hold, found at startup.
        let dir = crate::assets::asset_manager::test_support::audio_test_install("qm-files");
        std::fs::write(dir.path().join("rulesmd.ini"), b"[General]\n").unwrap();
        let game = crate::assets::asset_manager::AssetManager::new(
            dir.path(),
            MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap();
        changes.install_native_ini_hashes(&game, &[]);
        assert_eq!(changes.changed_game_files(), ["rulesmd.ini"]);
        assert_eq!(original_game_refusal(None, &changes), Some(CHANGED_FILES));
        // Ares On as well: the rules-differ text names both, and Quick Match
        // the setting, as On can mount the `ares.mix` a files box would name.
        changes = changes.with_ares_setting(AresSetting::On);
        assert_eq!(
            original_game_changes(None, &changes),
            [
                OriginalGameChange::Ares(AresSetting::On),
                OriginalGameChange::ChangedFiles(&["rulesmd.ini".to_owned()]),
            ]
        );
        assert_eq!(
            original_game_refusal(None, &changes),
            Some("VERA:QuickMatchAresSetting")
        );
        // Off opens the game Auto opens, so it is no change: the files are
        // what Quick Match names.
        changes = changes.with_ares_setting(AresSetting::Off);
        assert_eq!(
            original_game_changes(None, &changes),
            [OriginalGameChange::ChangedFiles(
                &["rulesmd.ini".to_owned()]
            )]
        );
        changes = changes.with_ares_setting(AresSetting::Auto);
        // With no mod selected, an installed `ares.mix` is ignored. The
        // changed rules still refuse Quick Match; removing them permits it.
        dir.write_mix("ares.mix", "ares.txt", b"ares");
        let game = crate::assets::asset_manager::AssetManager::new(
            dir.path(),
            MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap();
        changes.install_native_ini_hashes(&game, &[]);
        assert_eq!(changes.changed_game_files(), ["rulesmd.ini"]);
        assert_eq!(original_game_refusal(None, &changes), Some(CHANGED_FILES));
        std::fs::remove_file(dir.path().join("rulesmd.ini")).unwrap();
        let game = crate::assets::asset_manager::AssetManager::new(
            dir.path(),
            MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap();
        changes.install_native_ini_hashes(&game, &[]);
        assert!(changes.changed_game_files().is_empty());
        assert_eq!(original_game_refusal(None, &changes), None);
        // Original mode also skips an installed LANGRULE, even without a
        // valid Digest; it must not prevent Quick Match from queuing.
        std::fs::write(
            dir.path().join("langrule.ini"),
            b"[General]\nBuildSpeed=9\n",
        )
        .unwrap();
        let game = crate::assets::asset_manager::AssetManager::new(
            dir.path(),
            MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap();
        changes.install_native_ini_hashes(&game, &[]);
        assert!(changes.changed_game_files().is_empty());
        assert_eq!(original_game_refusal(None, &changes), None);
        // A selected mod remains ineligible for Quick Match.
        assert_eq!(
            original_game_refusal(Some("My Mod"), &changes),
            Some("VERA:QuickMatchNoMod")
        );
    }

    #[test]
    fn a_refused_join_says_the_games_differ() {
        use crate::ui::wol_lobby::{AdmissionContext, GAME_QUICK_MATCH, admit};
        let host = WolChannel {
            kind: GAME_CHANNEL,
            max_users: 2,
            users: 1,
            game_flags: GAME_QUICK_MATCH,
            name: "Host's_game".into(),
            ex_info: "25,7,0,0,0,x".into(),
            ..Default::default()
        };
        let same = AdmissionContext {
            quick_match: true,
            clan_id: 0,
            local_hash: Some(7),
            vera_rooms: true,
        };
        assert_eq!(admit(&host, false, same), Ok(()));
        let changed = AdmissionContext {
            local_hash: Some(8),
            ..same
        };
        let reason = admit(&host, false, changed).unwrap_err();
        assert_eq!(refusal_line(reason), Some("VERA:MatchStartMismatch"));
        assert_eq!(refusal_line(None), None);
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
