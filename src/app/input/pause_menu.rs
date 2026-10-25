//! In-game menu input. Native callback4F11B0 owns the commands of B5, BBA and
//! FF; rendering and hit-testing share the loaded side-art geometry
//! (72FC60,60B000,60B350,60B7A0).

use crate::app::match_runtime::session::{PauseEventLimiters, SessionMode, WallBucketTimer};
use crate::app::{App, AppState};
use crate::ui::pause_menu::InGameMenuState;
use crate::ui::shell::geom::ShellScale;
use crate::ui::shell::pause_menu::{
    InternetSlider, PauseMenuButton, PauseMenuButtonState, PauseMenuLayout, PauseMenuTemplate,
    pause_menu_layout,
};
use crate::ui::shell::trackbar::{TrackbarHold, TrackbarPress};
use crate::ui::skirmish_shell::SavedSeedMode;
use winit::event::MouseButton;

/// FF's two trackbars as the dialog holds them between init and Resume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct InternetPauseControls {
    /// Trackbar values in [`InternetSlider`] order.
    pub(crate) positions: [i32; 2],
    pub(crate) hold: Option<TrackbarHold<InternetSlider>>,
    pub(crate) hovered: Option<InternetSlider>,
}

impl InternetPauseControls {
    /// Init4F1720: `TBM_SETRANGE` then `TBM_SETPOS` with `3 - LatencyFudge`
    /// and `6 - GameSpeed`.
    pub(crate) fn open(latency_fudge: i32, game_speed: i32) -> Self {
        Self {
            positions: [
                InternetSlider::Connection.range().set_up(3 - latency_fudge),
                InternetSlider::Speed.range().set_up(6 - game_speed),
            ],
            ..Self::default()
        }
    }

    /// The right static's text: the CSF label for the current position.
    pub(crate) fn value_caption(&self, slider: InternetSlider) -> (&'static str, &'static str) {
        slider.value_caption(self.positions[slider as usize])
    }

    /// Store a pointer-selected position; a change is the trackbar's
    /// position notification (61E609..61E6DD).
    fn set(&mut self, slider: InternetSlider, position: i32) -> bool {
        let stored = &mut self.positions[slider as usize];
        if *stored == position {
            return false;
        }
        *stored = position;
        true
    }
}

/// The template the open in-game menu was created from.
pub(crate) fn current_template(state: &AppState) -> PauseMenuTemplate {
    state.match_state.match_presentation.pause_menu_template
}

/// `0x004F10E1..0x004F1130` for the running session.
fn select_template(state: &AppState) -> PauseMenuTemplate {
    let mode = state.match_state.session_mode();
    let local_defeated = state
        .match_state
        .sim_runtime
        .as_ref()
        .is_some_and(|runtime| runtime.simulation.local_player_defeated());
    PauseMenuTemplate::select(
        mode.is_single_player(),
        mode == SessionMode::Wol,
        local_defeated,
    )
}

pub(crate) fn current_layout(state: &AppState) -> Option<PauseMenuLayout> {
    let shell = crate::app::frontend::skirmish_shell_render::current_in_game_shell_layout(state)?;
    Some(pause_menu_layout(
        shell.width,
        shell.height,
        shell.layout,
        shell.button_size,
        current_template(state),
    ))
}

/// The applied GameSpeed FF's init (4F1720) reads; a queued speed event
/// changes it only when it executes.
fn current_game_speed(state: &AppState) -> i32 {
    state.match_state.sim_runtime.as_ref().map_or(0, |runtime| {
        runtime.simulation.session().game_options().game_speed
    })
}

/// Entering state 1 recreates the dialog: 4F10E0 picks its template and FF's
/// init reads the live values.
pub(crate) fn on_open(state: &mut AppState) {
    state.match_state.match_presentation.pause_menu_template = select_template(state);
    state.match_state.match_presentation.internet_pause = InternetPauseControls::open(
        state.match_state.session.latency_fudge(),
        current_game_speed(state),
    );
}

pub(crate) fn button_states(state: &AppState) -> [PauseMenuButtonState; 6] {
    let interaction = state.match_state.match_presentation.pause_menu_interaction;
    // Native4F17D9..4F1832 applies559C20 eligibility to Load/Delete only, and
    // only in modes0/5; BBA/FF have neither control.
    let has_saves = state.match_state.match_presentation.pause_menu_has_saves;
    PauseMenuButton::ALL.map(|button| PauseMenuButtonState {
        pressed: interaction.is_pressed(button),
        // C5 timer highlighting is separate from pointer hover.
        highlighted: false,
        enabled: has_saves || !matches!(button, PauseMenuButton::Load | PauseMenuButton::Delete),
    })
}

/// The status-line help for the control under the pointer.
pub(crate) fn hovered_help(state: &AppState) -> Option<&'static str> {
    let template = current_template(state);
    let presentation = &state.match_state.match_presentation;
    if let Some(slider) = presentation.internet_pause.hovered {
        return Some(slider.help());
    }
    template
        .control(presentation.pause_menu_interaction.hovered?)?
        .help
}

fn hit(state: &AppState, layout: &PauseMenuLayout) -> Option<PauseMenuButton> {
    let (x, y) = crate::app::frontend::skirmish_shell_render::in_game_shell_cursor(state);
    let states = button_states(state);
    layout
        .buttons()
        .find(|(button, rect)| states[*button as usize].enabled && rect.contains(x, y))
        .map(|(button, _)| button)
}

pub(crate) fn cursor_moved(state: &mut AppState) {
    let Some(layout) = current_layout(state) else {
        return;
    };
    let (x, y) = crate::app::frontend::skirmish_shell_render::in_game_shell_cursor(state);
    let over = hit(state, &layout);
    let pause = &mut state.match_state.match_presentation.internet_pause;
    // A held trackbar owns the capture (`0x0061E4DF`): nothing else sees the
    // pointer until release.
    if let Some(hold) = pause.hold {
        let changed = match (hold.dragging, layout.internet) {
            (true, Some(controls)) => {
                let rect = controls.sliders[hold.id as usize];
                let value = hold.id.range().value_at_in(x, rect, ShellScale::RETAIL);
                pause.set(hold.id, value)
            }
            _ => false,
        };
        if changed {
            App::play_skirmish_shell_generic_click_sound(state);
        }
        return;
    }
    pause.hovered = layout.internet.and_then(|c| c.slider_at(x, y));
    state
        .match_state
        .match_presentation
        .pause_menu_interaction
        .hovered = over;
}

pub(crate) fn mouse(state: &mut AppState, button: MouseButton, pressed: bool) {
    if button != MouseButton::Left {
        return;
    }
    let Some(layout) = current_layout(state) else {
        return;
    };
    let (x, y) = crate::app::frontend::skirmish_shell_render::in_game_shell_cursor(state);
    if !pressed
        && state
            .match_state
            .match_presentation
            .internet_pause
            .hold
            .take()
            .is_some()
    {
        return;
    }
    if pressed && let Some(controls) = layout.internet {
        if let Some(slider) = controls.slider_at(x, y) {
            let rect = controls.sliders[slider as usize];
            let pause = &mut state.match_state.match_presentation.internet_pause;
            let press = slider.range().press_in(
                pause.positions[slider as usize],
                x,
                y,
                rect,
                ShellScale::RETAIL,
            );
            pause.hold = press.hold(slider);
            if let TrackbarPress::Jump(value) = press
                && pause.set(slider, value)
            {
                App::play_skirmish_shell_generic_click_sound(state);
            }
            return;
        }
    }
    let over = hit(state, &layout);
    let interaction = &mut state.match_state.match_presentation.pause_menu_interaction;
    interaction.hovered = over;
    if pressed {
        interaction.press(over);
        if over.is_some() {
            // Owner-draw button-down: GUIMainButtonSound (Rules+0x188,
            // 0x0061374B..0x00613771).
            App::play_main_menu_button_sound(state);
        }
    } else if let Some(button) = interaction.release(over) {
        activate(state, button);
    }
}

fn activate(state: &mut AppState, button: PauseMenuButton) {
    match button {
        PauseMenuButton::GameControls => {
            App::enter_in_game_menu_state(state, InGameMenuState::Options)
        }
        PauseMenuButton::Resume => {
            resume_internet_events(state);
            App::enter_in_game_menu_state(state, InGameMenuState::Closed)
        }
        // 0x522 (`0x004F16C0..0x004F16EA`): GameMode 4 writes state 2 while
        // `[0xB779C4]` is set, every other game state 3.
        PauseMenuButton::Abort => {
            if state
                .match_state
                .sim_runtime
                .as_ref()
                .is_some_and(|runtime| runtime.simulation.session().wol_tournament_game())
            {
                crate::app::input::surrender::open(state);
            } else {
                App::enter_in_game_menu_state(state, InGameMenuState::AbortConfirm)
            }
        }
        PauseMenuButton::Load => App::open_saved_game_browser(state, SavedSeedMode::Load),
        PauseMenuButton::Save => App::open_saved_game_browser(state, SavedSeedMode::Save),
        PauseMenuButton::Delete => App::open_saved_game_browser(state, SavedSeedMode::Delete),
    }
}

/// What FF's Resume (`0x004F1534..0x004F16BB`) emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct InternetResume {
    pub(crate) latency_fudge: Option<i32>,
    pub(crate) game_speed: Option<i32>,
}

/// In mode4 with Session+0x300 clear, each changed trackbar emits its event
/// only while its limiter has expired, then restarts that limiter for 900
/// buckets; a refused change only logs. Speed is read after latency.
pub(crate) fn internet_resume(
    controls: &InternetPauseControls,
    latency_fudge: i32,
    game_speed: i32,
    limiters: &mut PauseEventLimiters,
    now_bucket: u32,
) -> InternetResume {
    let mut out = InternetResume::default();
    let fudge = 3 - controls.positions[InternetSlider::Connection as usize];
    if fudge != latency_fudge {
        if limiters.latency.remaining(now_bucket) == 0 {
            limiters.latency =
                WallBucketTimer::start(now_bucket, PauseEventLimiters::COOLDOWN_BUCKETS);
            out.latency_fudge = Some(fudge);
        } else {
            log::info!("Ignoring latency change to avoid congestion");
        }
    }
    let speed = 6 - controls.positions[InternetSlider::Speed as usize];
    if speed != game_speed {
        if limiters.speed.remaining(now_bucket) == 0 {
            limiters.speed =
                WallBucketTimer::start(now_bucket, PauseEventLimiters::COOLDOWN_BUCKETS);
            out.game_speed = Some(speed);
        } else {
            log::info!("Ignoring speed change to avoid congestion");
        }
    }
    out
}

/// Resume re-reads GameMode and Session+0x300 (4F1527..4F153B): a defeat
/// while FF is open drops both events. Only FF holds the trackbars.
fn resume_internet_events(state: &mut AppState) {
    let local_defeated = state
        .match_state
        .sim_runtime
        .as_ref()
        .is_some_and(|runtime| runtime.simulation.local_player_defeated());
    if current_template(state) != PauseMenuTemplate::Internet
        || state.match_state.session_mode() != SessionMode::Wol
        || local_defeated
    {
        return;
    }
    let controls = state.match_state.match_presentation.internet_pause;
    let latency_fudge = state.match_state.session.latency_fudge();
    let game_speed = current_game_speed(state);
    let now_bucket = crate::app::match_runtime::frame_pacer::wall_clock_bucket();
    let resume = internet_resume(
        &controls,
        latency_fudge,
        game_speed,
        state.match_state.session.pause_event_limiters_mut(),
        now_bucket,
    );
    let Some(owner) = crate::app::input::commands::preferred_local_owner(state) else {
        return;
    };
    // Resume4F1595..4F167A emits latency first, then speed. The limiters above
    // have already advanced even when OutList refuses either event.
    if let Some(value) = resume.latency_fudge {
        crate::app::input::commands::schedule_command(
            state,
            &owner,
            crate::sim::command::Command::SetLatencyFudge { value },
        );
    }
    if let Some(speed) = resume.game_speed {
        crate::app::input::commands::schedule_command(
            state,
            &owner,
            crate::sim::command::Command::LiveGameSpeed { speed: speed as u8 },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ff_init_projects_latency_and_speed_onto_the_trackbars() {
        let controls = InternetPauseControls::open(1, 3);
        assert_eq!(controls.positions, [2, 3]);
        // Init's range/position notifications set both labels at open.
        assert_eq!(
            controls.value_caption(InternetSlider::Connection).0,
            "TXT_GOOD_CONNECTION"
        );
        assert_eq!(
            controls.value_caption(InternetSlider::Speed).0,
            "TXT_MEDIUM"
        );
        // TBM_SETPOS ignores an out-of-range value.
        assert_eq!(InternetPauseControls::open(5, 9).positions, [0, 0]);
    }

    #[test]
    fn a_changed_position_updates_the_right_static() {
        let mut controls = InternetPauseControls::open(1, 3);
        assert!(!controls.set(InternetSlider::Speed, 3));
        assert!(controls.set(InternetSlider::Speed, 6));
        assert_eq!(
            controls.value_caption(InternetSlider::Speed).0,
            "TXT_FASTEST"
        );
        assert!(controls.set(InternetSlider::Connection, 0));
        assert_eq!(
            controls.value_caption(InternetSlider::Connection).0,
            "TXT_WORST_CONNECTION"
        );
    }

    #[test]
    fn resume_rate_limits_each_event_for_900_buckets() {
        let mut limiters = PauseEventLimiters::default();
        let mut controls = InternetPauseControls::open(1, 3);
        // Unchanged values emit nothing and arm nothing.
        assert_eq!(
            internet_resume(&controls, 1, 3, &mut limiters, 100),
            InternetResume::default()
        );
        controls.set(InternetSlider::Connection, 3);
        controls.set(InternetSlider::Speed, 5);
        assert_eq!(
            internet_resume(&controls, 1, 3, &mut limiters, 100),
            InternetResume {
                latency_fudge: Some(0),
                game_speed: Some(1),
            }
        );
        // Within 900 buckets both changes are refused.
        assert_eq!(
            internet_resume(&controls, 2, 4, &mut limiters, 999),
            InternetResume::default()
        );
        assert_eq!(
            internet_resume(&controls, 2, 4, &mut limiters, 1000),
            InternetResume {
                latency_fudge: Some(0),
                game_speed: Some(1),
            }
        );
    }
}
