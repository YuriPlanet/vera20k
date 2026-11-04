//! Which page owns the window: one ordered decision for render, the status
//! cards and the key, cursor, button and wheel paths, so a page is ordered in
//! one place. Top first:
//!
//! - The shell error card replaces every main-menu page, a sliding one
//!   included, so it takes input even while a transition runs.
//! - Two blocking layers take input without a page of their own: the house
//!   outcome wait (in game), then a shell transition or the quit cascade.
//! - Modal pages over the current screen. The in-game connection wait 0xEA
//!   sits over the Keyboard page A3: A3's pump (`0x005FBF2E`) runs
//!   `ProcessModalServicePump 0x00623120`, which in network modes enters
//!   `Main_Tick 0x0055D360` and from it `Wait_For_Players 0x00648710`, which
//!   opens 0xEA.
//! - The current screen's page: a main-menu overlay or route page, the
//!   loading screen, an in-game modal or the battlefield, or a score page.
//!   An in-game modal or a result without its retail page is a status card.

use crate::app::shell_route::ShellRoute;
use crate::app::{App, AppState, GameScreen};
use crate::ui::pause_menu::InGameMenuState;

/// A page drawn over the current screen, top first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModalPage {
    /// The main-menu shell error card.
    ShellError,
    NetworkFailure,
    /// 0xEA, the in-game connection wait.
    NetworkWait,
    /// A3, from the launcher or in-game Options.
    Keyboard,
}

/// The page that owns the current screen when no modal page is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScreenPage {
    FullscreenMovie,
    CreditsRoll,
    MovieList,
    Campaign,
    LanLobby,
    NetworkSetup,
    Wol,
    LoadSavedGame,
    /// Launcher Options 0xD5, over the main menu.
    LauncherOptions,
    Skirmish,
    SinglePlayer,
    MoviesAndCredits,
    /// The main menu 0xE2, also for a route whose page is not built.
    MainMenu,
    Loading,
    /// An in-scenario modal drawn as its active-game shell.
    InGameShell(InGameMenuState),
    /// An in-scenario modal without its retail art.
    InGameCard,
    /// The battlefield, under any popup it keeps drawing (0xCF, Surrender).
    Battlefield,
    CoopScore,
    CampaignScore,
    Score,
    /// A result with no native score page.
    ResultCard,
}

/// The layer that takes a window input event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputLayer {
    /// Takes every event; a release still drops held input state.
    HouseOutcomeWait,
    /// A shell slide or the quit cascade: no page takes input.
    ShellTransition,
    Page(ModalPage),
    Screen(ScreenPage),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Screen {
    #[default]
    MainMenu,
    Loading,
    InGame,
    MissionResult,
}

#[derive(Debug, Default, Clone, Copy)]
struct Layers {
    house_outcome_wait: bool,
    shell_transition: bool,
    shell_error: bool,
    network_failure: bool,
    network_wait: bool,
    keyboard: bool,
    screen: Screen,
    route: ShellRoute,
    fullscreen_movie: bool,
    credits_roll: bool,
    movie_list: bool,
    campaign: bool,
    wol: bool,
    load_saved_game: bool,
    launcher_options: bool,
    in_game_menu: InGameMenuState,
    /// The active-game shell art for `in_game_menu` is loaded.
    in_game_shell: bool,
    coop_score: bool,
    campaign_score: bool,
    score: bool,
}

impl Layers {
    fn of(state: &AppState) -> Self {
        let frontend = &state.frontend;
        Self {
            house_outcome_wait: App::house_outcome_wait_pending(state),
            shell_transition: crate::app::frontend::shell_transition::blocks_shell_input(state),
            shell_error: frontend.screen == GameScreen::MainMenu
                && frontend.main_menu_shell_error.is_some(),
            network_failure: crate::app::match_runtime::network_failure::pending(state),
            network_wait: frontend.screen == GameScreen::InGame
                && state.match_state.network_wait.dialog_open(),
            keyboard: frontend.keyboard_dialog.is_some(),
            screen: match frontend.screen {
                GameScreen::MainMenu => Screen::MainMenu,
                GameScreen::Loading => Screen::Loading,
                GameScreen::InGame => Screen::InGame,
                GameScreen::MissionResult { .. } => Screen::MissionResult,
            },
            route: frontend.shell_route,
            fullscreen_movie: frontend.fullscreen_movie.is_some(),
            credits_roll: frontend.credits_roll.is_some(),
            movie_list: App::movie_list_active(state),
            campaign: App::campaign_active(state),
            wol: App::wol_active(state),
            load_saved_game: App::load_saved_game_active(state),
            launcher_options: App::native_launcher_options_active(state),
            in_game_menu: state.match_state.match_presentation.in_game_menu,
            in_game_shell: crate::app::frontend::skirmish_shell_render::native_in_game_shell_active(
                state,
            ),
            coop_score: crate::app::input::coop_score::active(state),
            campaign_score: App::campaign_score_shell_active(state),
            score: App::score_shell_active(state),
        }
    }

    fn page(self) -> Option<ModalPage> {
        [
            (self.shell_error, ModalPage::ShellError),
            (self.network_failure, ModalPage::NetworkFailure),
            (self.network_wait, ModalPage::NetworkWait),
            (self.keyboard, ModalPage::Keyboard),
        ]
        .into_iter()
        .find_map(|(open, page)| open.then_some(page))
    }

    fn screen_page(self) -> ScreenPage {
        let first = |pages: &[(bool, ScreenPage)], otherwise| {
            pages
                .iter()
                .find_map(|&(open, page)| open.then_some(page))
                .unwrap_or(otherwise)
        };
        match self.screen {
            Screen::MainMenu => first(
                &[
                    // Play_Movie and Show_Credits run after their dialog is gone.
                    (self.fullscreen_movie, ScreenPage::FullscreenMovie),
                    (self.credits_roll, ScreenPage::CreditsRoll),
                    (self.movie_list, ScreenPage::MovieList),
                    (self.campaign, ScreenPage::Campaign),
                    (self.route.lan_lobby(), ScreenPage::LanLobby),
                    (self.route.network_setup(), ScreenPage::NetworkSetup),
                    (self.wol, ScreenPage::Wol),
                    (self.load_saved_game, ScreenPage::LoadSavedGame),
                    (self.launcher_options, ScreenPage::LauncherOptions),
                    (self.route.skirmish(), ScreenPage::Skirmish),
                    (self.route.single_player(), ScreenPage::SinglePlayer),
                    (
                        self.route.movies_and_credits(),
                        ScreenPage::MoviesAndCredits,
                    ),
                ],
                ScreenPage::MainMenu,
            ),
            Screen::Loading => ScreenPage::Loading,
            Screen::InGame => first(
                &[
                    (
                        self.in_game_shell,
                        ScreenPage::InGameShell(self.in_game_menu),
                    ),
                    (
                        self.in_game_menu.is_open() && !self.in_game_menu.draws_over_battlefield(),
                        ScreenPage::InGameCard,
                    ),
                ],
                ScreenPage::Battlefield,
            ),
            Screen::MissionResult => first(
                &[
                    (self.coop_score, ScreenPage::CoopScore),
                    (self.campaign_score, ScreenPage::CampaignScore),
                    (self.score, ScreenPage::Score),
                ],
                ScreenPage::ResultCard,
            ),
        }
    }

    fn input(self) -> InputLayer {
        if self.shell_error {
            InputLayer::Page(ModalPage::ShellError)
        } else if self.house_outcome_wait {
            InputLayer::HouseOutcomeWait
        } else if self.shell_transition {
            InputLayer::ShellTransition
        } else {
            self.page()
                .map_or_else(|| InputLayer::Screen(self.screen_page()), InputLayer::Page)
        }
    }
}

/// What render draws: the top modal page, and the screen's page under it.
pub(crate) fn presented(state: &AppState) -> (Option<ModalPage>, ScreenPage) {
    let layers = Layers::of(state);
    (layers.page(), layers.screen_page())
}

/// The modal page drawn over the current screen.
pub(crate) fn modal_page(state: &AppState) -> Option<ModalPage> {
    Layers::of(state).page()
}

/// The layer that takes a window input event now.
pub(crate) fn input_layer(state: &AppState) -> InputLayer {
    Layers::of(state).input()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The launcher A3 slides out with its dialog still retained: the slide
    /// takes a key as it takes every other input, while render still draws A3.
    #[test]
    fn a_shell_transition_takes_input_from_a_retained_keyboard_page() {
        let layers = Layers {
            shell_transition: true,
            keyboard: true,
            ..Default::default()
        };
        assert_eq!(layers.input(), InputLayer::ShellTransition);
        assert_eq!(layers.page(), Some(ModalPage::Keyboard));
    }

    /// 0xEA opens over in-game A3; it is drawn and takes input there.
    #[test]
    fn the_connection_wait_covers_the_in_game_keyboard_page() {
        let layers = Layers {
            network_wait: true,
            keyboard: true,
            ..Default::default()
        };
        assert_eq!(layers.input(), InputLayer::Page(ModalPage::NetworkWait));
        assert_eq!(layers.page(), Some(ModalPage::NetworkWait));
    }

    /// Input goes to the page render draws: a movie played from the movie
    /// list, a network failure over the art-less in-game menu card, and the
    /// shell error card that replaced a sliding page.
    #[test]
    fn input_goes_to_the_page_render_draws() {
        let movie = Layers {
            route: ShellRoute::MovieList,
            movie_list: true,
            fullscreen_movie: true,
            ..Default::default()
        };
        assert_eq!(movie.screen_page(), ScreenPage::FullscreenMovie);
        assert_eq!(
            movie.input(),
            InputLayer::Screen(ScreenPage::FullscreenMovie)
        );

        let mut menu = Layers {
            screen: Screen::InGame,
            in_game_menu: InGameMenuState::Menu,
            ..Default::default()
        };
        assert_eq!(menu.input(), InputLayer::Screen(ScreenPage::InGameCard));
        menu.network_failure = true;
        assert_eq!(menu.page(), Some(ModalPage::NetworkFailure));
        assert_eq!(menu.input(), InputLayer::Page(ModalPage::NetworkFailure));

        let stopped_slide = Layers {
            shell_error: true,
            shell_transition: true,
            route: ShellRoute::MovieList,
            movie_list: true,
            ..Default::default()
        };
        assert_eq!(stopped_slide.page(), Some(ModalPage::ShellError));
        assert_eq!(
            stopped_slide.input(),
            InputLayer::Page(ModalPage::ShellError)
        );
    }
}
