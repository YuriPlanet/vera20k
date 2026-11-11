//! ExecuteDoList64CCEB..64CD21's blocking checksum-error message box.
//! Simulation owns the suspended frame; this owner keeps only its local input
//! result. The shared shell controller owns button capture and keyboard routing.

use crate::app::{App, AppState};
use crate::sim::world::NetworkInterruption;
use crate::ui::shell::controller::{DialogController, ShellKey};
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::layout::LaidOutControl;
use crate::ui::shell::modal::{body_ok_layout_in, control};
use winit::event::{KeyEvent, MouseButton};
use winit::keyboard::PhysicalKey;

pub(crate) const DIALOG: DialogId = DialogId(0xCE);
pub(crate) const BODY: (&str, &str) = ("TXT_OUT_OF_SYNC", "Reconnection Error!");
pub(crate) const STOP: (&str, &str) = ("TXT_STOP", "Stop");

fn interruption(state: &AppState) -> Option<NetworkInterruption> {
    state
        .match_state
        .sim_runtime
        .runtime()
        .and_then(|rt| rt.simulation.pending_network_interruption())
}

pub(crate) fn labels(
    state: &AppState,
) -> ((&'static str, &'static str), (&'static str, &'static str)) {
    let body = match interruption(state) {
        Some(NetworkInterruption::ScenariosDiffer) => {
            ("TXT_SCENARIOS_DO_NOT_MATCH", "Scenarios do not match.")
        }
        Some(NetworkInterruption::QueueFull) => ("TXT_QUEUE_FULL", "Network event queue is full."),
        // A newer relay's record and a refused start end the match in the
        // same box; vera_body replaces its body.
        Some(
            NetworkInterruption::ConnectionFailed
            | NetworkInterruption::NewerClientRequired
            | NetworkInterruption::StartMismatch
            | NetworkInterruption::StartCancelled,
        ) => (
            "TXT_SYSTEM_NOT_RESPONDING",
            "The other system is not responding.",
        ),
        None => return (BODY, STOP),
    };
    //6480A7..64818A reads these body keys and825FB0=TXT_OK. The
    //checksum branch64CCEB uses TXT_STOP instead.
    (body, ("TXT_OK", "OK"))
}

/// VERA relay adaptation: no CSF string says that the relay sent records
/// this build cannot read, so the box names the cause and the download page.
/// An online match's connection failure is the relay's ordered stream, not
/// another player (their silence removes only their House), so it names the
/// game server rather than "the other system". A start the relay refused says
/// why, so the players can do something about it.
///
/// VERA-only: a frame that failed (`frame_stopped`, in any match) is an
/// error, not a desync, so the box says so and points at the log instead of
/// the checksum box's own out-of-sync text.
pub(crate) fn vera_body(state: &AppState) -> Option<String> {
    let mut body = reason(state)?;
    if state.match_state.frame_stopped
        && let Some(path) = crate::util::logging::log_file()
    {
        body.push_str("\n\n");
        body.push_str(&crate::i18n::tr_fill(
            "VERA:LogFileDetails",
            &[("path", &path.display())],
        ));
    }
    Some(body)
}

fn reason(state: &AppState) -> Option<String> {
    if state.match_state.frame_stopped {
        return Some(crate::i18n::tr("VERA:MatchInternalError"));
    }
    match interruption(state)? {
        NetworkInterruption::NewerClientRequired => Some(crate::i18n::tr_fill(
            "VERA:MatchNeedsNewerVera",
            &[("url", &crate::app::shell_wol::RELEASES_URL)],
        )),
        NetworkInterruption::ConnectionFailed if online(state) => {
            Some(crate::i18n::tr("VERA:GameServerConnectionFailed"))
        }
        NetworkInterruption::StartMismatch => Some(crate::i18n::tr("VERA:MatchStartMismatch")),
        NetworkInterruption::StartCancelled => Some(crate::i18n::tr("VERA:MatchStartCancelled")),
        _ => None,
    }
}

fn online(state: &AppState) -> bool {
    state.match_state.session_mode() == crate::app::match_runtime::session::SessionMode::Wol
}

/// VERA: the box closes with the match, so its reason also waits for the
/// chat the player lands in.
fn hold_chat_line(state: &mut AppState) {
    if !online(state) {
        return;
    }
    let line = reason(state).or_else(|| {
        let ((key, _), _) = labels(state);
        let csf = state.process_assets.csf.as_ref()?;
        Some(csf.get(key)?.to_string())
    });
    if let Some(line) = line {
        crate::app::shell_wol::hold_problem_line(state, line);
    }
}

/// VERA: Quit on the wait of a match the relay never started shows no box.
pub(crate) fn hold_quit_line(state: &mut AppState) {
    let never_started = state
        .match_state
        .live_network
        .as_ref()
        .and_then(|live| live.delivery_counters())
        .is_some_and(|counters| !counters.started);
    if online(state) && never_started {
        crate::app::shell_wol::hold_problem_line(
            state,
            crate::i18n::tr("VERA:MatchStartCancelled"),
        );
    }
}

#[derive(Debug, Default)]
pub(crate) struct NetworkFailureModal {
    acknowledged: bool,
}

impl NetworkFailureModal {
    fn key(&mut self, controller: &mut DialogController, key: ShellKey) {
        // ShellMessageBox5D3490 ignores the returned result at64CD11: Enter
        // and Escape both finish its single-button CE dialog.
        self.acknowledged |= controller.on_key(key);
    }

    fn mouse_up(
        &mut self,
        controller: &mut DialogController,
        x: i32,
        y: i32,
        feed: &[LaidOutControl],
    ) {
        self.acknowledged |= controller.on_pointer_up(x, y, feed) == Some(control::OK);
    }
}

pub(crate) fn pending(state: &AppState) -> bool {
    state.frontend.screen == crate::app::GameScreen::InGame
        && (state.match_state.frame_stopped
            || state.match_state.sim_runtime.runtime().is_some_and(|rt| {
                rt.simulation.pending_network_failure().is_some()
                    || rt.simulation.pending_network_interruption().is_some()
            }))
}

/// Also used for a restored suspended frame: no second mismatch record or
/// independent simulation gate is kept in the app.
pub(crate) fn show(state: &mut AppState) {
    if state.match_state.network_failure.is_none() {
        if let Some(failure) = state
            .match_state
            .sim_runtime
            .runtime()
            .and_then(|rt| rt.simulation.pending_network_failure())
        {
            log::error!("{failure}");
        }
        if let Some(failure) = state
            .match_state
            .sim_runtime
            .runtime()
            .and_then(|rt| rt.simulation.pending_network_interruption())
        {
            log::error!("{failure}");
        }
        state.match_state.network_failure = Some(NetworkFailureModal::default());
        state.match_state.input.cancel_tactical_drag();
        state.match_state.input.keys_held.clear();
        state
            .match_state
            .match_presentation
            .in_game_gadgets
            .cancel_pointer_capture();
    }
    state.frontend.shell_controller.ensure_active(DIALOG, true);
    state.platform.window.request_redraw();
}

pub(crate) fn take_acknowledged(state: &mut AppState) -> bool {
    if !state
        .match_state
        .network_failure
        .as_ref()
        .is_some_and(|modal| modal.acknowledged)
    {
        return false;
    }
    hold_chat_line(state);
    state.match_state.network_failure = None;
    if state.frontend.shell_controller.top_id() == Some(DIALOG) {
        state.frontend.shell_controller.pop();
    }
    true
}

/// The box keeps gamemd's centring on the match's frame.
fn button_feed(width: i32, height: i32) -> [LaidOutControl; 1] {
    [LaidOutControl {
        id: control::OK,
        rect: body_ok_layout_in(RectPx::new(0, 0, width, height)).ok,
    }]
}

pub(crate) fn key(state: &mut AppState, event: &KeyEvent) {
    show(state);
    if let PhysicalKey::Code(code) = event.physical_key {
        if !event.state.is_pressed() {
            state.match_state.input.keys_held.remove(&code);
        } else if !event.repeat
            && let Some(key) = App::shell_key_for_code(code)
        {
            state
                .match_state
                .network_failure
                .as_mut()
                .expect("shown modal")
                .key(&mut state.frontend.shell_controller, key);
        }
    }
}

pub(crate) fn cursor_moved(state: &mut AppState) {
    show(state);
    let feed = button_feed(state.render_width() as i32, state.render_height() as i32);
    let (x, y) = state.match_state.input.cursor_px();
    state.frontend.shell_controller.on_pointer_move(x, y, &feed);
}

pub(crate) fn mouse(state: &mut AppState, button: MouseButton, pressed: bool) {
    show(state);
    if button != MouseButton::Left {
        return;
    }
    let feed = button_feed(state.render_width() as i32, state.render_height() as i32);
    let (x, y) = state.match_state.input.cursor_px();
    if pressed {
        state.frontend.shell_controller.on_pointer_down(x, y, &feed);
        // The box's owner-draw OK plays GUIMainButtonSound (0x0061376B).
        if state.frontend.shell_controller.pressed().is_some() {
            App::play_main_menu_button_sound(state);
        }
    } else {
        state
            .match_state
            .network_failure
            .as_mut()
            .expect("shown modal")
            .mouse_up(&mut state.frontend.shell_controller, x, y, &feed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_stop_requires_the_shared_press_and_release_capture() {
        let mut controller = DialogController::default();
        controller.ensure_active(DIALOG, true);
        let mut modal = NetworkFailureModal::default();
        let feed = button_feed(800, 600);
        let rect = feed[0].rect;
        let (x, y) = (rect.x + 1, rect.y + 1);
        modal.mouse_up(&mut controller, x, y, &feed);
        assert!(!modal.acknowledged);
        controller.on_pointer_down(x, y, &feed);
        modal.mouse_up(&mut controller, 0, 0, &feed);
        assert!(!modal.acknowledged);
        controller.on_pointer_down(x, y, &feed);
        modal.mouse_up(&mut controller, x, y, &feed);
        assert!(modal.acknowledged);
    }

    #[test]
    fn checksum_message_uses_the_shared_single_button_key_results() {
        for key in [ShellKey::Enter, ShellKey::Escape] {
            let mut controller = DialogController::default();
            controller.ensure_active(DIALOG, true);
            let mut modal = NetworkFailureModal::default();
            modal.key(&mut controller, ShellKey::Tab);
            assert!(!modal.acknowledged);
            modal.key(&mut controller, key);
            assert!(modal.acknowledged);
        }
    }
}
