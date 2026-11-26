//! The Quick Match and Buddy List runners' entries into the shared lobby
//! records: a new runner, its joins, creates, boxes and leaves, the buddy
//! dialog's open and resume.

use super::created_channel;
use crate::app::shell_wol::WolPage;
use crate::app::shell_wol::service::WolServiceRequest;
use crate::app::{App, AppState};
use crate::ui::wol_lobby::*;
use crate::util::time::Instant;

use super::{
    AfterLeave, BoxThen, GAMES_PERIOD, LEAVE_WAIT, LIST_WAIT, LOBBIES_PERIOD, QuickMatchBoxThen,
    Runner, Step, WolLobby, admission_context, close_runner, continue_join, current_channel,
    ex_info, join_channel, join_failed_locally, leave_then_create, lobby_mut, nickname,
    options_screen, request, show_box, show_wait,
};
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::session::MatchSession;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::shell_main_menu::ClientStep;
use crate::app::shell_wol::WolVisit;
use crate::render::gpu::GpuContext;
use crate::util::config::GameConfig;
use winit::window::Window;

pub(super) fn new_runner(
    runner: Runner,
    observer: bool,
    postmatch: bool,
    now: Instant,
    modern: bool,
) -> Box<WolLobby> {
    Box::new(WolLobby {
        runner,
        ui: WolLobbyState::new(observer, modern),
        games: Vec::new(),
        lobbies: Vec::new(),
        members: Vec::new(),
        current: None,
        pending: None,
        shown: false,
        covered: false,
        wait: None,
        message: None,
        step: Step::ListingLobbies {
            deadline: now + LIST_WAIT,
        },
        postmatch,
        games_at: now + GAMES_PERIOD,
        lobbies_at: now + LOBBIES_PERIOD,
        refresh_at: None,
        deferred: None,
        joining: None,
        matches: Vec::new(),
        watch_list_stale: false,
        visit: None,
    })
}

/// A runner with no `0x113` and the chat-lobby list behind
/// `TXT_REQ_CHANLIST` (`0x0077EF90(0, 0)`, the 30 s wait `0x0077F120`): the
/// Buddy List runner's entry (`0x0079F140`, steps 1-2) and the Quick Match
/// runner's lobby list (`0x0079FCF0..0x0079FD35`), whose entry cleared the
/// observer preference (`0x0079FC40..0x0079FC5D`).
pub(in crate::app::shell_wol) fn open_runner(
    frontend: &mut FrontendState,
    session: &mut MatchSession,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    options_profile: &RetailOptionsProfile,
    runner: Runner,
) {
    let observer = frontend.multiplayer_shell_runtime.lan_observer();
    let Some(visit) = frontend.wol.as_mut() else {
        return;
    };
    visit.in_game = false;
    visit.page = WolPage::Lobby;
    visit.lobby = Some(new_runner(
        runner,
        observer,
        false,
        Instant::now(),
        !visit.legacy_ui,
    ));
    if session.network_return_context().is_none() {
        session.enter_network(crate::app::match_runtime::session::NetworkReturnContext::Wol);
    }
    super::leaf::free_art(&mut frontend.wol_art, crate::ui::wol_login::DIALOG);
    frontend.shell_route = crate::app::shell_route::ShellRoute::Wol;
    let _ = App::ensure_skirmish_shell_chrome(renderer, frontend, process_assets, options_profile);
    show_wait(frontend, process_assets, "TXT_REQ_CHANLIST");
    request(
        frontend.wol.as_mut(),
        WolServiceRequest::ListChannels { kind: 0, flags: 0 },
    );
}

/// `0x00788800` for the Quick Match lobby (stage 2): a lobby already
/// joined or pending answers 0 without a request. The join's step is
/// returned.
#[must_use]
pub(in crate::app::shell_wol) fn quick_match_join(
    frontend: &mut FrontendState,
    window: &Window,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    options_profile: &RetailOptionsProfile,
    channel: WolChannel,
    key: &str,
) -> Option<ClientStep> {
    join_channel(
        frontend,
        window,
        match_state,
        process_assets,
        renderer,
        options_profile,
        channel,
        key,
        false,
        false,
    )
}

/// `0x00789960(record, DL=0)` for the host's game the guest found: the
/// admission `0x0079BA60` with Quick Match's ranked test, then the join.
/// RESIDUAL: `DL=0`'s internals are not traced; the Quick Match game has
/// no key and no resolution bits, so neither popup can follow. The join's
/// step is returned.
#[must_use]
pub(in crate::app::shell_wol) fn quick_match_join_game(
    frontend: &mut FrontendState,
    window: &Window,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    options_profile: &RetailOptionsProfile,
    channel: WolChannel,
) -> Option<ClientStep> {
    let context = admission_context(frontend.wol.as_ref(), process_assets);
    if let Err(reason) = admit(&channel, false, context) {
        // VERA: the log keeps what was compared; gamemd records nothing.
        log::warn!(
            "WOL Quick Match: {} refused the join ({}); local rules hash {:?}, host exInfo {:?}",
            channel.name,
            reason.unwrap_or("no message"),
            context.local_hash,
            channel.ex_info,
        );
        super::quick_match::game_refused(frontend, match_state, process_assets, renderer, reason);
        return None;
    }
    join_channel(
        frontend,
        window,
        match_state,
        process_assets,
        renderer,
        options_profile,
        channel,
        "",
        true,
        false,
    )
}

/// `0x00789B60`'s Quick Match branch (`0x00789BA8..0x00789D60`,
/// `0x00789EC4..0x00789EEE`): no `0xC3` and no key; the leave and the
/// create then run as for Create.
pub(in crate::app::shell_wol) fn quick_match_create(
    frontend: &mut FrontendState,
    game_config: Option<&GameConfig>,
    session: &MatchSession,
    process_assets: &ProcessAssets,
    gpu: &GpuContext,
    options_profile: &RetailOptionsProfile,
) {
    let nickname = nickname(&frontend.wol_account);
    let tournament = i32::from(!super::quick_match::quick_coop(frontend.wol.as_ref()));
    let (width, _) = options_screen(gpu, options_profile);
    let ex_info = ex_info(frontend, session, process_assets);
    let globals = &mut frontend.wol_lobby_globals;
    if width >= 1024 {
        globals.limit_resolution = false;
    }
    let channel = quick_match_channel(&nickname, tournament, globals.lobby_number, ex_info);
    leave_then_create(
        frontend,
        game_config,
        process_assets,
        channel,
        String::new(),
        false,
        true,
    );
}

/// `0x00789B60`'s record in Quick Match: Create's record and exInfo
/// (`0x0077E7A0` at `0x00789D05` runs before the mode branch) with two users
/// (`[0xB77DC4] = 2`, `0x00789BE0`), tournament 1 (Quick Co-op 0,
/// `0x00789EC9`) and game flags `0x800` plus `[0xB77DD4]`: no clan or
/// resolution bits.
pub(super) fn quick_match_channel(
    nickname: &str,
    tournament: i32,
    lobby_number: i32,
    ex_info: String,
) -> WolChannel {
    WolChannel {
        max_users: 2,
        tournament,
        game_flags: GAME_QUICK_MATCH.wrapping_add(lobby_number as u32),
        ..created_channel(nickname, false, false, 0, lobby_number, ex_info)
    }
}

/// A box of the Quick Match runner over `0x117` or the backdrop.
pub(in crate::app::shell_wol) fn quick_match_box(
    frontend: &mut FrontendState,
    process_assets: &ProcessAssets,
    key: &str,
    then: QuickMatchBoxThen,
) {
    show_box(
        frontend,
        process_assets,
        key,
        false,
        BoxThen::QuickMatch(then),
    );
}

/// The runner's exit leave (`0x007A03A0..0x007A0410`): a channel this user
/// is in is left behind `WOL:LeavingChannel` (at most 150 x 100 ms);
/// then the runner returns 0.
pub(in crate::app::shell_wol) fn quick_match_leave_and_return(state: &mut AppState) {
    let Some(lobby) = lobby_mut(state.frontend.wol.as_mut()) else {
        close_runner(state);
        return;
    };
    lobby.pending = None;
    if lobby.current.is_none() {
        close_runner(state);
        return;
    }
    lobby.step = Step::Leaving {
        deadline: Instant::now() + LEAVE_WAIT,
        then: AfterLeave::Return,
    };
    request(state.frontend.wol.as_mut(), WolServiceRequest::LeaveChannel);
    show_wait(
        &mut state.frontend,
        &state.process_assets,
        "WOL:LeavingChannel",
    );
}

/// The Buddy List runner's dialog loop takes over: its records stay, the
/// runner's own boxes are closed and the backdrop shows under `0x116`.
pub(in crate::app::shell_wol) fn buddy_dialog_opens(wol: Option<&mut WolVisit>) {
    if let Some(lobby) = lobby_mut(wol) {
        lobby.wait = None;
        lobby.message = None;
        lobby.step = Step::Ready;
        lobby.covered = false;
        lobby.ui.close_popup();
        lobby.joining = None;
    }
}

/// The Buddy List runner's dialog loop after a match or a stopped
/// Create/Join (C2/C9 Back left the game channel).
pub(in crate::app::shell_wol) fn buddy_runner_resumes(wol: Option<&mut WolVisit>) {
    if let Some(lobby) = lobby_mut(wol) {
        // A game channel C2 Back is still leaving stays until its OnPart.
        if lobby
            .current
            .as_ref()
            .is_none_or(|current| current.kind != GAME_CHANNEL)
        {
            lobby.current = None;
            lobby.members.clear();
        }
        lobby.pending = None;
        lobby.shown = false;
    }
}

/// C2 Back's own leave (API+0x24 at `0x007900B8`), after the dialog closed.
pub(crate) fn host_setup_left(mut wol: Option<&mut WolVisit>) {
    if current_channel(wol.as_deref()).is_some_and(|current| current.kind == GAME_CHANNEL) {
        request(wol.as_deref_mut(), WolServiceRequest::LeaveChannel);
    }
}

/// Join from `0x116` for a buddy in a listed game: `0x00789960` with the
/// record, over the empty backdrop. Its own admission `0x0079BA60` runs
/// first: `0x116`'s observer box does not recompute Join's enable, so the
/// game may refuse here; then `0x00789960` returns false and the loop goes
/// on. RESIDUAL: its refusal line (`show` = 1) goes to a list that is not
/// traced while `0x116` is down; none is printed. The join's step is
/// returned.
#[must_use]
pub(in crate::app::shell_wol) fn buddy_join(
    frontend: &mut FrontendState,
    window: &Window,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    options_profile: &RetailOptionsProfile,
    channel: WolChannel,
) -> Option<ClientStep> {
    if let Some(lobby) = lobby_mut(frontend.wol.as_mut()) {
        lobby.step = Step::Ready;
        lobby.shown = false;
    }
    let observer = frontend.multiplayer_shell_runtime.lan_observer();
    if admit(
        &channel,
        observer,
        admission_context(frontend.wol.as_ref(), process_assets),
    )
    .is_err()
    {
        join_failed_locally(
            frontend,
            window,
            match_state,
            process_assets,
            renderer,
            options_profile,
            true,
        );
        return None;
    }
    continue_join(
        frontend,
        window,
        match_state,
        process_assets,
        renderer,
        options_profile,
        channel,
    )
}

/// Join from `0x116` for a game the list lacks: type `0x29` again behind
/// `TXT_REQ_CHANLIST` (`0x0077EF90(0x29, 0)`, `0x0077F120`), then a
/// second search.
pub(in crate::app::shell_wol) fn buddy_find_game(
    frontend: &mut FrontendState,
    process_assets: &ProcessAssets,
    name: String,
) {
    if let Some(lobby) = lobby_mut(frontend.wol.as_mut()) {
        lobby.step = Step::FindingGame {
            name,
            deadline: Instant::now() + LIST_WAIT,
        };
    }
    show_wait(frontend, process_assets, "TXT_REQ_CHANLIST");
    request(
        frontend.wol.as_mut(),
        WolServiceRequest::ListChannels {
            kind: GAME_CHANNEL,
            flags: 0,
        },
    );
}
