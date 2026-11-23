//! ExecuteDoList64CCEB..64CD21's blocking checksum-error message box.
//! Simulation owns the suspended frame; this owner keeps only its local input
//! result. The shared shell controller owns button capture and keyboard routing.

use crate::app::AppState;
use crate::app::input::dialog_page;
use crate::sim::world::NetworkInterruption;
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::modal::{control, message_box_parts};
use crate::ui::shell::page::PageEvent;
use vera20k_gui::{InputEvent, NodeKind, NodeSpec, Prop, Value};
use winit::event::{KeyEvent, MouseButton};
use winit::keyboard::PhysicalKey;

pub(crate) const DIALOG: DialogResourceId = DialogResourceId(0xCE);
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
            | NetworkInterruption::StartCancelled
            | NetworkInterruption::FederationLost,
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
            &[("url", &crate::app::web_link::DOWNLOAD_PAGE)],
        )),
        NetworkInterruption::ConnectionFailed if online(state) => {
            Some(crate::i18n::tr("VERA:GameServerConnectionFailed"))
        }
        NetworkInterruption::FederationLost => Some(crate::i18n::tr("VERA:FederationLost")),
        NetworkInterruption::StartMismatch => Some(start_mismatch_text(state)),
        NetworkInterruption::StartCancelled => Some(crate::i18n::tr("VERA:MatchStartCancelled")),
        _ => None,
    }
}

/// VERA: a refused start also names what this game changes from the
/// original game, the terrain and picture or model files this match read and
/// the Ares setting Off where it kept Ares from playing among them, or says
/// it plays the original (`agreement::start_changes_text`), and a launcher's
/// match points at the launcher, which compares every player's rules parts
/// (docs/spawn.md). Those parts leave the terrain files, the picture and
/// model files and that Ares answer out, so a match that read such files or
/// has that answer does not point there.
fn start_mismatch_text(state: &AppState) -> String {
    use crate::app::shell_network_setup::agreement::{
        match_keeps_ares_off, match_picture_files, start_changes_text,
    };
    let mut text = crate::i18n::tr("VERA:MatchStartMismatch");
    text.push_str("\n\n");
    text.push_str(
        &start_changes_text(state).unwrap_or_else(|| crate::i18n::tr("VERA:RulesDifferOriginal")),
    );
    if state.frontend.external_launch
        && !state.process_assets.match_reads_folder_terrain()
        && !match_keeps_ares_off(state)
        && match_picture_files(state).is_empty()
    {
        text.push('\n');
        text.push_str(&crate::i18n::tr("VERA:LauncherShowsMismatch"));
    }
    text
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
    /// One input on the box: OK acknowledges once released over its own
    /// press. ShellMessageBox5D3490 ignores the returned result at64CD11:
    /// Enter and Escape both finish its single-button CE dialog, so OK is
    /// its default and cancel button.
    fn input(
        &mut self,
        controller: &mut DialogController,
        event: InputEvent,
        ok: RectPx,
    ) -> Vec<PageEvent> {
        let nodes = || {
            NodeSpec::new(NodeKind::Dialog).child(
                NodeSpec::new(NodeKind::Button)
                    .id(control::OK.into())
                    .with(Prop::Default, Value::Bool(true))
                    .with(Prop::Cancel, Value::Bool(true)),
            )
        };
        let rect = |id| (id == control::OK).then_some(ok);
        let events =
            dialog_page::route(controller, DIALOG, nodes, rect, dialog_page::no_sync, event);
        self.acknowledged |= events.contains(&PageEvent::Activated(control::OK));
        events
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
    state.frontend.shell_controller.ensure_active(DIALOG);
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
fn ok_rect(width: i32, height: i32) -> RectPx {
    message_box_parts(1, RectPx::new(0, 0, width, height)).button(0)
}

/// Whether OK shows pushed.
pub(crate) fn ok_pressed(state: &AppState) -> bool {
    let controller = &state.frontend.shell_controller;
    controller.top_id() == Some(DIALOG) && dialog_page::pressed(controller, control::OK)
}

pub(crate) fn key(state: &mut AppState, event: &KeyEvent) {
    show(state);
    if let PhysicalKey::Code(code) = event.physical_key {
        if !event.state.is_pressed() {
            state.match_state.input.keys_held.remove(&code);
        } else if !event.repeat
            && let Some(key) = crate::ui::shell::controller::gui_key(code, false)
        {
            route(state, InputEvent::Key(key));
        }
    }
}

fn pointer(state: &mut AppState, down: Option<bool>) {
    show(state);
    let (x, y) = state.match_state.input.cursor_px();
    route(state, dialog_page::event_at(x, y, down));
}

fn route(state: &mut AppState, event: InputEvent) {
    let ok = ok_rect(state.render_width() as i32, state.render_height() as i32);
    let events = state
        .match_state
        .network_failure
        .as_mut()
        .expect("shown modal")
        .input(&mut state.frontend.shell_controller, event, ok);
    // The box's owner-draw OK plays GUIMainButtonSound (0x0061376B).
    dialog_page::play_cues(state, events);
}

pub(crate) fn cursor_moved(state: &mut AppState) {
    pointer(state, None);
}

pub(crate) fn mouse(state: &mut AppState, button: MouseButton, pressed: bool) {
    if button == MouseButton::Left {
        pointer(state, Some(pressed));
    } else {
        show(state);
    }
}

#[cfg(all(test, tests_app_match))]
mod tests {
    use super::*;

    #[test]
    fn checksum_stop_requires_the_shared_press_and_release_capture() {
        let mut controller = DialogController::default();
        controller.ensure_active(DIALOG);
        let mut modal = NetworkFailureModal::default();
        let rect = ok_rect(800, 600);
        let (x, y) = (rect.x + 1, rect.y + 1);
        let mut send = |modal: &mut NetworkFailureModal, x, y, down| {
            modal.input(&mut controller, dialog_page::event_at(x, y, down), rect);
        };
        send(&mut modal, x, y, None);
        send(&mut modal, x, y, Some(false));
        assert!(!modal.acknowledged);
        send(&mut modal, x, y, Some(true));
        send(&mut modal, 0, 0, Some(false));
        assert!(!modal.acknowledged);
        send(&mut modal, x, y, None);
        send(&mut modal, x, y, Some(true));
        send(&mut modal, x, y, Some(false));
        assert!(modal.acknowledged);
    }

    #[test]
    fn checksum_message_uses_the_shared_single_button_key_results() {
        use vera20k_gui::Key;
        for key in [Key::Enter, Key::Escape] {
            let mut controller = DialogController::default();
            controller.ensure_active(DIALOG);
            let mut modal = NetworkFailureModal::default();
            let ok = ok_rect(800, 600);
            modal.input(&mut controller, InputEvent::Key(Key::Tab), ok);
            assert!(!modal.acknowledged);
            modal.input(&mut controller, InputEvent::Key(key), ok);
            assert!(modal.acknowledged);
        }
    }
}
