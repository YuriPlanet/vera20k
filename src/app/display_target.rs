//! Which screen the game opens on, and moving the window onto it.
//!
//! The Options page's Display list is the only chooser. This module owns the
//! one translation between a [`DisplayTarget`] (the profile's own value) and
//! the window system's monitors, so the list, the launch attributes and the
//! live move all agree on what a saved name means.
//!
//! Two kinds of target. A saved screen name points at one screen of the
//! Options list. [`DisplayTarget::Auto`] names no screen: the window stays
//! where the system puts it, as in every build before this option. A name the
//! desktop no longer lists answers nothing in the same way.
//!
//! VERA's own option: `[VERA]` is VERA's section and no engine reads it, and
//! the list sits on the Vera options page rather than the retail page whose
//! layout this repository ports.
//!
//! Presentation-only: nothing here is serialized into a replay, hashed, or read
//! by the deterministic simulation.

use winit::dpi::PhysicalPosition;
use winit::monitor::MonitorHandle;
use winit::window::{Fullscreen, Window};

use crate::ui::main_menu_dialogs::options::{DisplayChoice, DisplayTarget};

/// Every attached screen, in the window system's enumeration order, as the
/// Options list shows them.
///
/// A screen the backend cannot name is skipped: the choice is persisted by
/// name, so an entry with no name could never be picked again.
pub(crate) fn attached_displays(
    monitors: impl Iterator<Item = MonitorHandle>,
) -> Vec<MonitorChoice> {
    monitors
        .filter_map(|monitor| {
            let id = monitor.name()?;
            let size = monitor.size();
            let refresh_millihertz = monitor.refresh_rate_millihertz();
            Some(MonitorChoice {
                handle: monitor,
                choice: DisplayChoice {
                    id,
                    width: size.width,
                    height: size.height,
                    refresh_millihertz,
                },
            })
        })
        .collect()
}

/// A screen as the Options list shows it (`choice`) plus the live handle that
/// places the window on it.
pub(crate) struct MonitorChoice {
    pub(crate) handle: MonitorHandle,
    pub(crate) choice: DisplayChoice,
}

impl MonitorChoice {
    /// What the Options list needs, without the platform handle.
    pub(crate) fn choice(&self) -> &DisplayChoice {
        &self.choice
    }
}

/// The screens this window can be placed on, for the Options list.
///
/// `Window::available_monitors` is `ActiveEventLoop::available_monitors`
/// (`winit` 0.30's own note), so the window alone is enough and the list can be
/// rebuilt without threading the event loop through the shell.
pub(crate) fn current_displays(window: &Window) -> Vec<MonitorChoice> {
    attached_displays(window.available_monitors())
}

/// The same list before a window exists, for the launch attributes.
pub(crate) fn current_displays_for(
    event_loop: &winit::event_loop::ActiveEventLoop,
) -> Vec<MonitorChoice> {
    attached_displays(event_loop.available_monitors())
}

/// The index of the screen a target means, among `ids`.
///
/// [`DisplayTarget::Auto`] answers nothing, so the window system keeps its
/// own placement. A saved name answers the screen it names, matched whole:
/// `\\.\DISPLAY1` is not `\\.\DISPLAY10`, and the Windows device names are
/// upper case, so a hand-edited lower-case name is a different device. A name
/// the desktop no longer lists answers nothing.
fn target_index<'a>(
    ids: impl IntoIterator<Item = &'a str>,
    target: &DisplayTarget,
) -> Option<usize> {
    match target {
        DisplayTarget::Auto => None,
        DisplayTarget::Fixed(wanted) => ids.into_iter().position(|id| id == wanted),
    }
}

/// The screen a target names while that screen is attached; `None` for
/// [`DisplayTarget::Auto`] or a name the desktop no longer lists, which leaves
/// the window where the system puts it.
pub(crate) fn screen_for(
    displays: &[MonitorChoice],
    target: &DisplayTarget,
) -> Option<MonitorHandle> {
    let index = target_index(
        displays.iter().map(|display| display.choice.id.as_str()),
        target,
    )?;
    Some(displays[index].handle.clone())
}

/// Where the window sits on `monitor` when it is not full screen: centred, the
/// same way the window manager would place a window the player just opened.
///
/// The centring happens in physical pixels against the monitor's own rectangle,
/// so a mixed-DPI desktop keeps the window on the screen it was moved to. A
/// window larger than that screen starts at its corner instead of hanging off
/// it, so the player can still reach the whole frame.
pub(crate) fn centered_outer_position(
    window: &Window,
    monitor: &MonitorHandle,
) -> PhysicalPosition<i32> {
    let monitor_size = monitor.size();
    let monitor_origin = monitor.position();
    let window_size = window.outer_size();
    PhysicalPosition::new(
        monitor_origin.x + (monitor_size.width as i32 - window_size.width as i32).max(0) / 2,
        monitor_origin.y + (monitor_size.height as i32 - window_size.height as i32).max(0) / 2,
    )
}

/// Move the windowed window onto `target`'s screen, centred.
///
/// The windowed half of [`apply`], for callers that already know the window is
/// not full screen and must not ask it: `winit`'s full-screen change is applied
/// on its own thread, so the window's own answer is stale for a while after one.
pub(crate) fn center_windowed(window: &Window, displays: &[MonitorChoice], target: &DisplayTarget) {
    let Some(monitor) = screen_for(displays, target) else {
        return;
    };
    window.set_outer_position(centered_outer_position(window, &monitor));
}

/// Whether the window already sits on the screen `target` means.
///
/// True when the target answers no screen at all: there is nothing to move it
/// to, which is always the case for Auto.
pub(crate) fn window_is_on(
    window: &Window,
    displays: &[MonitorChoice],
    target: &DisplayTarget,
) -> bool {
    let Some(chosen) = screen_for(displays, target) else {
        return true;
    };
    window
        .current_monitor()
        .is_some_and(|current| current == chosen)
}

/// Move the window onto `target`.
///
/// Full screen is the window's own state, so this follows it: a full-screen
/// window changes which screen its borderless surface covers, and a windowed
/// one is moved and centred.
///
/// A full screen names its screen, because `winit` reads a borderless full
/// screen without a monitor as "the window's current screen" and then leaves
/// the window where it is. Naming it is what moves the surface.
///
/// A window already full screen leaves that state and enters it again on the
/// new screen, rather than naming the new screen to `set_fullscreen` in one
/// step. `winit` 0.30.12 stores the placement it will restore on leaving full
/// screen on *every* entry (Windows: `GetWindowPlacement` before
/// `SetWindowPos`), so a single step would save the full-screen rectangle as
/// that placement: leaving full screen later would then put a screen-sized
/// window back, on the screen the player left. Leaving first restores the real
/// windowed placement, and the entry that follows saves that one.
pub(crate) fn apply(window: &Window, displays: &[MonitorChoice], target: &DisplayTarget) {
    if window.fullscreen().is_none() {
        center_windowed(window, displays, target);
        return;
    }
    let Some(monitor) = screen_for(displays, target) else {
        return;
    };
    window.set_fullscreen(None);
    window.set_fullscreen(Some(Fullscreen::Borderless(Some(monitor))));
}

/// The full-screen request for `target`: the screen the target means, or the
/// system's own choice when a saved name is gone.
pub(crate) fn fullscreen_for(displays: &[MonitorChoice], target: &DisplayTarget) -> Fullscreen {
    Fullscreen::Borderless(screen_for(displays, target))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> [&'static str; 3] {
        ["\\\\.\\DISPLAY1", "\\\\.\\DISPLAY2", "\\\\.\\DISPLAY3"]
    }

    #[test]
    fn a_named_screen_resolves_to_its_place_in_the_list() {
        assert_eq!(
            target_index(
                ids().iter().copied(),
                &DisplayTarget::Fixed("\\\\.\\DISPLAY1".to_owned())
            ),
            Some(0)
        );
        assert_eq!(
            target_index(
                ids().iter().copied(),
                &DisplayTarget::Fixed("\\\\.\\DISPLAY3".to_owned())
            ),
            Some(2)
        );
    }

    #[test]
    fn a_screen_name_that_is_gone_resolves_to_nothing() {
        // Unplugging the saved screen must not stop the game from starting, and
        // must not silently rewrite the choice: the profile keeps the name.
        assert_eq!(
            target_index(
                ids().iter().copied(),
                &DisplayTarget::Fixed("\\\\.\\DISPLAY9".to_owned())
            ),
            None
        );
    }

    #[test]
    fn matching_is_exact_rather_than_prefix_based() {
        // DISPLAY1 is a prefix of DISPLAY10, so a prefix match would put the
        // window on the wrong screen on a desktop with ten outputs.
        let many = ["\\\\.\\DISPLAY10", "\\\\.\\DISPLAY1"];
        assert_eq!(
            target_index(
                many.iter().copied(),
                &DisplayTarget::Fixed("\\\\.\\DISPLAY1".to_owned())
            ),
            Some(1)
        );
        // A case difference is a different device string: Windows upper-cases
        // these names, and an INI edited by hand in lower case is not a match.
        assert_eq!(
            target_index(
                many.iter().copied(),
                &DisplayTarget::Fixed("\\\\.\\display1".to_owned())
            ),
            None
        );
    }

    #[test]
    fn auto_leaves_the_window_to_the_system() {
        // A profile that never picked a screen keeps the placement every
        // earlier build had: nothing is resolved, so nothing is moved.
        assert_eq!(
            target_index(ids().iter().copied(), &DisplayTarget::Auto),
            None
        );
    }

    #[test]
    fn no_attached_screens_answers_the_system_for_every_target() {
        // A headless session: nothing resolves, and Auto is the only behaviour
        // left, which the callers handle as a plain no-op.
        assert_eq!(
            target_index(
                [].iter().copied(),
                &DisplayTarget::Fixed("\\\\.\\DISPLAY1".to_owned())
            ),
            None
        );
        assert_eq!(target_index([].iter().copied(), &DisplayTarget::Auto), None);
    }
}
