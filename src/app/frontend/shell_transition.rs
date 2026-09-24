//! Generic shell first-paint slide driver (menu / single-player / skirmish).
//!
//! The original plays a controls-reveal animation on the *first paint of every
//! allow-listed shell dialog* — not a menu->skirmish edge transition and not a
//! whole-screen crossfade. Each owner-draw control's chrome-SHP frame index
//! advances on a staggered 30 ms-per-frame schedule; controls are never
//! repositioned.
//!
//! The render-agnostic data + schedule live in [`crate::ui::shell::slide`] (the
//! dialog-id allow-list, per-dialog animated-slot count, and the [`ShellFrameWave`]
//! frame sweep). This module is the app/render glue: it maps the currently-showing
//! screen to a shell dialog, (re)starts/advances the wave on entry edges, plays
//! the slide-in start cue, and dispatches the per-frame shell repaint while the
//! wave is live. The slide-in start cue is `GUIMoveInSound` (stock `MenuSlideIn`);
//! the stock-empty end cue (`ShellButtonSlideSound`) stays silent.

use std::time::Instant;

use anyhow::{Result, bail};

use crate::app::AppState;
use crate::ui::shell::descriptor::DialogId;

// Re-export the render-agnostic schedule types from the shared substrate so the
// shell renderers (and the `AppState` field) keep their existing import paths.
pub(crate) use crate::ui::shell::slide::{
    ButtonGroup, MainMenuEntryPaintFrame, MainMenuEntryPresentToken, PresentedPoll, ShellFrameWave,
};

pub(crate) enum ShellFirstPaintRenderResult {
    NotRendered,
    Rendered {
        main_menu_entry_token: Option<MainMenuEntryPresentToken>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MainMenuFirstPaintPoll {
    Acquire,
    WaitUntil(Instant),
    Completed,
}

/// Which shell dialog a first-paint slide belongs to. Every allow-listed shell
/// dialog slides on its own first paint; this identifies the one currently
/// showing so the trigger can detect entry edges and look up the control count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShellSlideKind {
    /// Dialog 0xE2 — main menu (6 owner-draw buttons).
    MainMenu,
    /// Dialog 0x100 — single-player shell (4 owner-draw buttons).
    SinglePlayer,
    /// Dialog 0x101 — Movies & Credits (4 owner-draw buttons).
    MoviesAndCredits,
    /// Dialog 0x129 — movie list (Play Movie and Back).
    MovieList,
    /// Dialog 0x102 — offline skirmish setup (3 right-panel buttons).
    Skirmish,
}

impl ShellSlideKind {
    /// The Win32 dialog resource id this shell maps to. The slide's eligibility
    /// and animated-slot count are looked up from this id in the data-driven
    /// `slide` table (no hardcoded per-kind counts here).
    pub(crate) fn dialog_id(self) -> DialogId {
        DialogId(match self {
            ShellSlideKind::MainMenu => 0x00E2,
            ShellSlideKind::SinglePlayer => 0x0100,
            ShellSlideKind::MoviesAndCredits => 0x0101,
            ShellSlideKind::MovieList => 0x0129,
            ShellSlideKind::Skirmish => 0x0102,
        })
    }

    /// Number of animated owner-draw button slots, which sets the stagger length.
    /// Sourced from the data-driven `slide` table; every rendered shell has an
    /// entry, so a miss is a programming error.
    fn slot_count(self) -> u32 {
        crate::ui::shell::slide::slot_count_for(self.dialog_id())
            .expect("rendered shell dialog must have a slide slot count")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellEntryEffect {
    Unchanged,
    LeftShells,
    Started(ShellSlideKind),
}

#[derive(Debug)]
enum ShellWaveCompletion {
    /// Single Player `0x100`, Movies & Credits `0x101` or movie list `0x129`.
    MenuPage,
    Skirmish,
}

/// Render-agnostic reducer for dialog-instance, entry-wave, and title state.
///
/// Production route mutations and the frame driver both use this reducer, so a
/// dialog destroyed between paints cannot be hidden from an edge detector that
/// only remembers the last rendered target.
struct ShellLifecycleReducer<'a> {
    active_shell: &'a mut Option<ShellSlideKind>,
    first_paint_slide: &'a mut Option<ShellFrameWave>,
    slide_generation: &'a mut u64,
    title_reveal: &'a mut crate::ui::shell::static_reveal::Kind1StaticReveal,
}

impl<'a> ShellLifecycleReducer<'a> {
    fn from_state(state: &'a mut AppState) -> Self {
        Self {
            active_shell: &mut state.frontend.shell_slide_active_shell,
            first_paint_slide: &mut state.frontend.shell_first_paint_slide,
            slide_generation: &mut state.frontend.shell_slide_generation,
            title_reveal: &mut state.frontend.main_menu_shell_state.title_reveal,
        }
    }

    fn invalidate_main_menu_dialog_instance(&mut self) {
        self.title_reveal.reset_waiting();
        *self.active_shell = None;
        *self.first_paint_slide = None;
    }

    fn observe_target(&mut self, target: Option<ShellSlideKind>, now: Instant) -> ShellEntryEffect {
        if target == *self.active_shell {
            return ShellEntryEffect::Unchanged;
        }
        if target == Some(ShellSlideKind::MainMenu)
            || *self.active_shell == Some(ShellSlideKind::MainMenu)
        {
            self.title_reveal.reset_waiting();
        }
        *self.active_shell = target;
        match target {
            Some(kind) => {
                *self.first_paint_slide = Some(if kind == ShellSlideKind::MainMenu {
                    *self.slide_generation = self.slide_generation.wrapping_add(1);
                    if *self.slide_generation == 0 {
                        *self.slide_generation = 1;
                    }
                    ShellFrameWave::new_presented_main_menu(*self.slide_generation)
                } else {
                    ShellFrameWave::new_first_paint_slide(kind.slot_count(), now)
                });
                ShellEntryEffect::Started(kind)
            }
            None => {
                *self.first_paint_slide = None;
                ShellEntryEffect::LeftShells
            }
        }
    }

    fn advance_wave(&mut self, now: Instant) {
        if let Some(wave) = self.first_paint_slide.as_mut() {
            wave.advance(now);
        }
    }

    fn finish_completed_wave(&mut self, kind: ShellSlideKind) -> Option<ShellWaveCompletion> {
        if !self
            .first_paint_slide
            .as_ref()
            .is_some_and(ShellFrameWave::is_complete)
        {
            return None;
        }
        *self.first_paint_slide = None;
        Some(match kind {
            ShellSlideKind::MainMenu => return None,
            ShellSlideKind::SinglePlayer
            | ShellSlideKind::MoviesAndCredits
            | ShellSlideKind::MovieList => ShellWaveCompletion::MenuPage,
            ShellSlideKind::Skirmish => ShellWaveCompletion::Skirmish,
        })
    }

    fn complete_presented_main_menu(&mut self, generation: u64, title: &str, now: Instant) -> bool {
        if *self.active_shell != Some(ShellSlideKind::MainMenu)
            || !self
                .first_paint_slide
                .as_ref()
                .is_some_and(|wave| wave.is_presented_completing(generation))
        {
            if let Some(wave) = self.first_paint_slide.as_mut() {
                wave.poison_presented();
            }
            return false;
        }
        if !self.title_reveal.start(title, now)
            || !self
                .first_paint_slide
                .as_ref()
                .is_some_and(|wave| wave.is_presented_completing(generation))
        {
            if let Some(wave) = self.first_paint_slide.as_mut() {
                wave.poison_presented();
            }
            return false;
        }
        *self.first_paint_slide = None;
        true
    }
}

/// Advance the Skirmish right-panel static text reveals by one cadence step.
/// Call once per frame: the per-label advance is internally 30 ms-gated and a
/// no-op while no reveal is active, so an unconditional per-frame call never
/// over-advances. This lives outside `render_shell_first_paint_slide` because
/// the reveals start *at* the slide's completion edge (when the slide clears)
/// and keep animating afterwards, when that renderer no longer runs.
pub(crate) fn advance_shell_static_reveals(state: &mut AppState) {
    state
        .frontend.skirmish_shell_state
        .advance_right_panel_static_reveals(Instant::now());
}

/// Invalidate the destroyed/recreated 0xE2 dialog instance at an actual route
/// boundary, even when the destination never reaches a paint.
///
/// `shell_slide_active_shell` remembers the last target observed by the frame
/// driver. Clearing it here makes a collapsed 0xE2 -> 0x100 -> Back round trip
/// produce a fresh 0xE2 entry edge instead of inheriting the old terminal title.
pub(crate) fn invalidate_main_menu_dialog_instance(state: &mut AppState) {
    ShellLifecycleReducer::from_state(state).invalidate_main_menu_dialog_instance();
}

/// Deliver the kind-1 timer only while the bare 0xE2 dialog owns steady paint.
/// Waiting state is inert, so the terminal slide frame cannot begin the title
/// before its own successful presentation.
pub(crate) fn poll_main_menu_title_reveal(state: &mut AppState) {
    if current_shell_slide_target(state) == Some(ShellSlideKind::MainMenu)
        && state.frontend.shell_first_paint_slide.is_none()
    {
        state
            .frontend.main_menu_shell_state
            .title_reveal
            .poll_timer(Instant::now());
    }
}

pub(crate) fn current_main_menu_entry_frame(state: &AppState) -> Option<MainMenuEntryPaintFrame> {
    state
        .frontend.shell_first_paint_slide
        .as_ref()
        .and_then(ShellFrameWave::current_main_menu_frame)
}

/// Commit one exact main-menu generation/tick after `output.present()`.
/// Any impossible mismatch poisons the already-visible wave before returning.
pub(crate) fn record_main_menu_entry_presented(
    state: &mut AppState,
    token: MainMenuEntryPresentToken,
) -> Result<()> {
    let generation = token.generation();
    let tick = token.tick();
    if current_shell_slide_target(state) != Some(ShellSlideKind::MainMenu) {
        if let Some(wave) = state.frontend.shell_first_paint_slide.as_mut() {
            wave.poison_presented();
        }
        bail!(
            "main-menu present token {generation}:{tick} committed after route ownership changed"
        );
    }
    let Some(wave) = state.frontend.shell_first_paint_slide.as_mut() else {
        bail!("main-menu present token {generation}:{tick} has no active wave");
    };
    if let Err(error) = wave.record_presented(token, Instant::now()) {
        wave.poison_presented();
        bail!("main-menu present token {generation}:{tick} rejected: {error}");
    }
    Ok(())
}

pub(crate) fn blocks_shell_input(state: &AppState) -> bool {
    // The graceful quit cascade also freezes shell input (the original processes
    // no input during its blocking teardown), so a stray click can't re-enter the
    // menu mid-fade.
    state.frontend.quit_cascade.is_some()
        || transition_blocks_shell_input(state.frontend.shell_first_paint_slide.as_ref())
}

pub(crate) fn transition_blocks_shell_input(transition: Option<&ShellFrameWave>) -> bool {
    transition.is_some()
}

pub(crate) fn main_menu_presented_wake_deadline(state: &AppState) -> Option<Instant> {
    state
        .frontend.shell_first_paint_slide
        .as_ref()
        .and_then(ShellFrameWave::presented_wake_deadline)
}

pub(crate) fn main_menu_presented_is_poisoned(state: &AppState) -> bool {
    state
        .frontend.shell_first_paint_slide
        .as_ref()
        .is_some_and(ShellFrameWave::is_presented_poisoned)
}

/// Which allow-listed shell dialog is currently showing, if any. Mirrors the
/// main-menu render dispatch order (skirmish > single-player > bare menu); the
/// egui fallback / skirmish-setup paths are not native shell dialogs and do not
/// slide. The candidate is gated through the data-driven slide allow-list, so a
/// dialog only slides when its id is eligible. Returns `None` off the main menu
/// screen.
pub(crate) fn current_shell_slide_target(state: &AppState) -> Option<ShellSlideKind> {
    use crate::ui::game_screen::GameScreen;
    if state.frontend.screen != GameScreen::MainMenu {
        return None;
    }
    // Play_Movie and Show_Credits run after their source dialog is destroyed
    // and before the next one exists: no shell dialog is showing.
    if state.frontend.fullscreen_movie.is_some() || state.frontend.credits_roll.is_some() {
        return None;
    }
    let candidate =
        if state.frontend.shell_route.skirmish() || state.frontend.dev_skirmish_shell_enabled {
            ShellSlideKind::Skirmish
        } else if state.frontend.shell_route.single_player() {
            ShellSlideKind::SinglePlayer
        } else if state.frontend.shell_route.movies_and_credits() {
            ShellSlideKind::MoviesAndCredits
        } else if state.frontend.shell_route.movie_list() {
            ShellSlideKind::MovieList
        } else if !state.frontend.main_menu_shell_failed {
            ShellSlideKind::MainMenu
        } else {
            return None;
        };
    crate::ui::shell::slide::is_slide_eligible(candidate.dialog_id()).then_some(candidate)
}

/// Arm a newly created `0xE2` before swapchain acquisition. This deliberately
/// does not play the entry cue or expose tick 0.
pub(crate) fn prepare_main_menu_first_paint_before_acquire(state: &mut AppState) {
    let target = current_shell_slide_target(state);
    if target == Some(ShellSlideKind::MainMenu) {
        if state.frontend.shell_slide_active_shell != target {
            ShellLifecycleReducer::from_state(state).observe_target(target, Instant::now());
        }
    } else if state.frontend.shell_slide_active_shell == Some(ShellSlideKind::MainMenu) {
        ShellLifecycleReducer::from_state(state).observe_target(None, Instant::now());
    }
}

/// Poll the exact `0xE2` clock before acquiring another surface.
pub(crate) fn poll_main_menu_first_paint_before_acquire(
    state: &mut AppState,
    now: Instant,
) -> Result<MainMenuFirstPaintPoll> {
    if current_shell_slide_target(state) != Some(ShellSlideKind::MainMenu) {
        return Ok(MainMenuFirstPaintPoll::Acquire);
    }
    let Some(wave) = state.frontend.shell_first_paint_slide.as_mut() else {
        return Ok(MainMenuFirstPaintPoll::Acquire);
    };
    let Some(poll) = wave.poll_presented(now) else {
        return Ok(MainMenuFirstPaintPoll::Acquire);
    };
    match poll {
        PresentedPoll::Acquire => Ok(MainMenuFirstPaintPoll::Acquire),
        PresentedPoll::WaitUntil(deadline) => Ok(MainMenuFirstPaintPoll::WaitUntil(deadline)),
        PresentedPoll::Poisoned => bail!("main-menu first-paint wave is poisoned"),
        PresentedPoll::Complete => {
            let generation = wave
                .presented_generation()
                .expect("presented poll has generation");
            // Active-retail stock leaves ShellButtonSlideSound empty, but the
            // completion hook remains a named lifecycle edge.
            crate::app::App::play_shell_slide_completion_sound(state);
            let title = crate::app::frontend::main_menu_shell_render::main_menu_title_text(state).into_owned();
            if !ShellLifecycleReducer::from_state(state)
                .complete_presented_main_menu(generation, &title, now)
            {
                bail!("main-menu generation {generation} failed its completion transaction");
            }
            Ok(MainMenuFirstPaintPoll::Completed)
        }
    }
}

/// Activate an armed `0xE2` only after successful swapchain acquisition.
/// Compatibility dialogs retain their prior post-acquisition entry behavior.
pub(crate) fn activate_shell_first_paint_after_acquire(state: &mut AppState) {
    let target = current_shell_slide_target(state);
    if target == Some(ShellSlideKind::MainMenu) {
        if state
            .frontend.shell_first_paint_slide
            .as_mut()
            .is_some_and(ShellFrameWave::activate_after_acquire)
        {
            crate::app::App::play_shell_slide_in_sound(state);
        }
        return;
    }
    let effect = ShellLifecycleReducer::from_state(state).observe_target(target, Instant::now());
    if let ShellEntryEffect::Started(_) = effect {
        crate::app::App::play_shell_slide_in_sound(state);
    }
}

/// Render the currently-showing shell while its first-paint slide is live, then
/// advance/complete the wave. Returns `Rendered` when it owned the frame. The shell
/// renderer reads `state.shell_first_paint_slide` and swaps each owner-draw
/// button's SDBTNANM frame index — controls are never repositioned, and the rest
/// of the shell paints exactly as it does steady-state.
pub(crate) fn render_shell_first_paint_slide(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<ShellFirstPaintRenderResult> {
    if state.frontend.shell_first_paint_slide.is_none() {
        return Ok(ShellFirstPaintRenderResult::NotRendered);
    }
    let Some(kind) = current_shell_slide_target(state) else {
        // No eligible shell is showing; drop the stale wave and let the normal
        // dispatch paint this frame.
        state.frontend.shell_first_paint_slide = None;
        return Ok(ShellFirstPaintRenderResult::NotRendered);
    };

    if kind == ShellSlideKind::MainMenu {
        let frame = current_main_menu_entry_frame(state)
            .ok_or_else(|| anyhow::anyhow!("main-menu entry acquired without a ready frame"))?;
        return match crate::app::frontend::main_menu_shell_render::render_main_menu_first_paint_frame(
            state,
            encoder,
            destination,
            frame,
        )? {
            crate::app::frontend::main_menu_shell_render::MainMenuEntryRenderResult::Rendered { token } => {
                Ok(ShellFirstPaintRenderResult::Rendered {
                    main_menu_entry_token: Some(token),
                })
            }
            crate::app::frontend::main_menu_shell_render::MainMenuEntryRenderResult::Fallback => {
                state.frontend.shell_first_paint_slide = None;
                Ok(ShellFirstPaintRenderResult::NotRendered)
            }
        };
    }

    ShellLifecycleReducer::from_state(state).advance_wave(Instant::now());

    let rendered = match kind {
        ShellSlideKind::Skirmish => {
            if !crate::app::App::ensure_skirmish_shell_chrome(state) {
                log::warn!("Skirmish shell chrome unavailable; cancelling first-paint slide");
                state.frontend.shell_first_paint_slide = None;
                return Ok(ShellFirstPaintRenderResult::NotRendered);
            }
            let color = state.renderer.shell_surface_presenter.source_render_view();
            let depth = state.renderer.depth_view.clone();
            crate::app::frontend::skirmish_shell_render::render_skirmish_shell_to_target(
                state,
                encoder,
                crate::render::shell_transition_pass::ShellRenderTarget {
                    color: &color,
                    depth: &depth,
                },
                crate::app::frontend::skirmish_shell_render::ShellRenderMode::TransitionPreview,
            )?;
            state
                .renderer.shell_surface_presenter
                .encode_present(encoder, destination);
            true
        }
        ShellSlideKind::MovieList => {
            crate::app::frontend::movies_credits_render::render_movie_list(
                state,
                encoder,
                destination,
            )?
        }
        ShellSlideKind::SinglePlayer | ShellSlideKind::MoviesAndCredits => matches!(
            crate::app::frontend::menu_page_render::render_active_menu_page(
                state,
                encoder,
                destination,
            )?,
            crate::app::frontend::menu_page_render::MenuPageRenderResult::Rendered
        ),
        ShellSlideKind::MainMenu => unreachable!("handled above"),
    };

    if !rendered {
        // Shell fell back (assets missing): abandon the slide so the normal
        // dispatch can render the fallback path with its egui overlays.
        state.frontend.shell_first_paint_slide = None;
        return Ok(ShellFirstPaintRenderResult::NotRendered);
    }

    let completion = ShellLifecycleReducer::from_state(state).finish_completed_wave(kind);
    match completion {
        // The slide completion edge kicks off the Skirmish right-panel statics'
        // character reveal. Start it here, on the same edge that clears the
        // slide, using the strings the renderer will draw.
        Some(ShellWaveCompletion::Skirmish) => {
            let now = Instant::now();
            let (title, game_type, map_label) =
                crate::app::frontend::skirmish_shell_render::skirmish_right_panel_label_strings(state);
            state
                .frontend.skirmish_shell_state
                .start_right_panel_static_reveals(&title, &game_type, &map_label, now);
        }
        Some(ShellWaveCompletion::MenuPage) | None => {}
    }

    Ok(ShellFirstPaintRenderResult::Rendered {
        main_menu_entry_token: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::shell::static_reveal::{Kind1PaintWindow, Kind1StaticReveal};
    use std::time::Duration;

    #[test]
    fn shell_kinds_map_to_their_dialog_ids() {
        assert_eq!(ShellSlideKind::MainMenu.dialog_id(), DialogId(0x00E2));
        assert_eq!(ShellSlideKind::SinglePlayer.dialog_id(), DialogId(0x0100));
        assert_eq!(ShellSlideKind::Skirmish.dialog_id(), DialogId(0x0102));
    }

    #[test]
    fn shell_kinds_resolve_data_driven_slot_counts() {
        assert_eq!(ShellSlideKind::MainMenu.slot_count(), 5);
        assert_eq!(ShellSlideKind::SinglePlayer.slot_count(), 4);
        assert_eq!(ShellSlideKind::MoviesAndCredits.slot_count(), 4);
        assert_eq!(ShellSlideKind::MovieList.slot_count(), 2);
        assert_eq!(ShellSlideKind::Skirmish.slot_count(), 3);
    }

    #[test]
    fn gsi_13_26_menu_page_first_paint_uses_same_presenter_entrypoint() {
        let source = include_str!("shell_transition.rs");
        let production = source
            .split_once("#[cfg(test)]")
            .expect("test module follows production transition renderer")
            .0;
        let renderer = &production[production
            .find("pub(crate) fn render_shell_first_paint_slide")
            .expect("production first-paint renderer")..];
        let branch = &renderer[renderer
            .find("ShellSlideKind::SinglePlayer | ShellSlideKind::MoviesAndCredits =>")
            .expect("menu-page first-paint branch")..];
        let branch = branch
            .split_once("ShellSlideKind::MainMenu")
            .map_or(branch, |(branch, _)| branch);

        assert!(branch.contains("render_active_menu_page"));
        assert!(branch.contains("destination"));
        assert!(!branch.contains("target"));
    }

    #[test]
    fn collapsed_e2_to_100_back_before_paint_rearms_title_and_entry_wave() {
        let start = Instant::now();
        let mut title_reveal = Kind1StaticReveal::default();
        assert!(title_reveal.start("Main Menu", start));
        for count in 1..=17 {
            let Kind1PaintWindow::Due { window, receipt } = title_reveal.paint_window() else {
                panic!("expected dirty title paint {count}");
            };
            assert_eq!(window.count, count);
            assert!(title_reveal.record_presented(receipt));
            if count < 17 {
                assert!(
                    title_reveal.poll_timer(start + Duration::from_millis(30 * u64::from(count)))
                );
            }
        }
        assert!(title_reveal.is_terminal_persistent());

        let mut active_shell = Some(ShellSlideKind::MainMenu);
        let mut first_paint_slide = None;
        let mut slide_generation = 41;
        let mut slide_sound_edges = Vec::new();

        // Open 0x100: 0xE2 is destroyed, but no 0x100 frame is allowed to run.
        ShellLifecycleReducer {
            active_shell: &mut active_shell,
            first_paint_slide: &mut first_paint_slide,
            slide_generation: &mut slide_generation,
            title_reveal: &mut title_reveal,
        }
        .invalidate_main_menu_dialog_instance();
        assert_eq!(title_reveal.paint_window(), Kind1PaintWindow::Hidden);
        assert_eq!(active_shell, None);
        assert!(first_paint_slide.is_none());
        assert!(slide_sound_edges.is_empty());

        // Queued Back destroys 0x100 and recreates 0xE2 before the frame driver.
        ShellLifecycleReducer {
            active_shell: &mut active_shell,
            first_paint_slide: &mut first_paint_slide,
            slide_generation: &mut slide_generation,
            title_reveal: &mut title_reveal,
        }
        .invalidate_main_menu_dialog_instance();
        assert!(first_paint_slide.is_none());
        assert!(slide_sound_edges.is_empty());

        // The next production frame observes only the recreated 0xE2. No 0x100
        // wave or sound ever existed; exactly one fresh 0xE2 entry edge does.
        let effect = ShellLifecycleReducer {
            active_shell: &mut active_shell,
            first_paint_slide: &mut first_paint_slide,
            slide_generation: &mut slide_generation,
            title_reveal: &mut title_reveal,
        }
        .observe_target(Some(ShellSlideKind::MainMenu), start);
        assert_eq!(effect, ShellEntryEffect::Started(ShellSlideKind::MainMenu));
        assert_eq!(slide_generation, 42);
        assert!(slide_sound_edges.is_empty(), "arming must remain silent");
        assert_eq!(title_reveal.paint_window(), Kind1PaintWindow::Hidden);
        assert!(first_paint_slide.is_some());
        assert_eq!(active_shell, Some(ShellSlideKind::MainMenu));

        // Successful acquisition activates tick 0 and is the one sound edge.
        let wave = first_paint_slide.as_mut().expect("armed main-menu wave");
        assert!(wave.activate_after_acquire());
        slide_sound_edges.push(ShellSlideKind::MainMenu);
        assert_eq!(
            wave.current_main_menu_frame().map(|frame| frame.tick()),
            Some(0)
        );

        let mut accepted_at = start;
        for expected_tick in 0..=crate::ui::shell::slide::MAIN_MENU_TERMINAL_TICK {
            let wave = first_paint_slide.as_mut().expect("active main-menu wave");
            let frame = wave.current_main_menu_frame().expect("ready frame");
            assert_eq!((frame.generation(), frame.tick()), (42, expected_tick));
            let token = wave.mint_present_token(frame).expect("matching token");
            wave.record_presented(token, accepted_at).expect("commit");
            if expected_tick < crate::ui::shell::slide::MAIN_MENU_TERMINAL_TICK {
                accepted_at += Duration::from_millis(30);
                assert_eq!(
                    wave.poll_presented(accepted_at),
                    Some(PresentedPoll::Acquire)
                );
            }
        }
        let wave = first_paint_slide
            .as_mut()
            .expect("terminal hold remains active");
        assert_eq!(
            wave.poll_presented(accepted_at + Duration::from_millis(29)),
            Some(PresentedPoll::WaitUntil(
                accepted_at + Duration::from_millis(30)
            ))
        );
        assert_eq!(
            wave.poll_presented(accepted_at + Duration::from_millis(30)),
            Some(PresentedPoll::Complete)
        );
        assert!(first_paint_slide.is_some(), "completing still blocks input");
        assert_eq!(title_reveal.paint_window(), Kind1PaintWindow::Hidden);

        let completion_at = accepted_at + Duration::from_millis(30);
        let mut completion_events = vec!["ShellButtonSlideSound"];
        assert!(
            ShellLifecycleReducer {
                active_shell: &mut active_shell,
                first_paint_slide: &mut first_paint_slide,
                slide_generation: &mut slide_generation,
                title_reveal: &mut title_reveal,
            }
            .complete_presented_main_menu(42, "Main Menu", completion_at)
        );
        completion_events.push("title-start");
        completion_events.push("clear");
        assert_eq!(
            completion_events,
            ["ShellButtonSlideSound", "title-start", "clear"]
        );
        assert!(first_paint_slide.is_none());
        let Kind1PaintWindow::Due { window, .. } = title_reveal.paint_window() else {
            panic!("recreated title did not begin a fresh reveal");
        };
        assert_eq!(window.count, 1);
        assert_eq!(slide_sound_edges, [ShellSlideKind::MainMenu]);
    }

    #[test]
    fn completion_generation_mismatch_poison_prevents_retry() {
        let start = Instant::now();
        let mut active_shell = Some(ShellSlideKind::MainMenu);
        let mut first_paint_slide = Some(ShellFrameWave::new_presented_main_menu(52));
        let mut slide_generation = 52;
        let mut title_reveal = Kind1StaticReveal::default();

        let wave = first_paint_slide.as_mut().expect("presented wave");
        assert!(wave.activate_after_acquire());
        let mut accepted_at = start;
        for expected_tick in 0..=crate::ui::shell::slide::MAIN_MENU_TERMINAL_TICK {
            let frame = wave.current_main_menu_frame().expect("ready frame");
            assert_eq!(frame.tick(), expected_tick);
            let token = wave.mint_present_token(frame).expect("matching token");
            wave.record_presented(token, accepted_at).expect("commit");
            accepted_at += Duration::from_millis(30);
            let expected_poll = if expected_tick < crate::ui::shell::slide::MAIN_MENU_TERMINAL_TICK
            {
                PresentedPoll::Acquire
            } else {
                PresentedPoll::Complete
            };
            assert_eq!(wave.poll_presented(accepted_at), Some(expected_poll));
        }

        assert!(
            !ShellLifecycleReducer {
                active_shell: &mut active_shell,
                first_paint_slide: &mut first_paint_slide,
                slide_generation: &mut slide_generation,
                title_reveal: &mut title_reveal,
            }
            .complete_presented_main_menu(51, "Main Menu", accepted_at)
        );
        let wave = first_paint_slide.as_mut().expect("poisoned wave remains");
        assert!(wave.is_presented_poisoned());
        assert_eq!(
            wave.poll_presented(accepted_at),
            Some(PresentedPoll::Poisoned),
            "a failed completion transaction must never replay its hook"
        );
        assert_eq!(title_reveal.paint_window(), Kind1PaintWindow::Hidden);
    }
}
