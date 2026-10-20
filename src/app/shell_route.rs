//! Frontend shell route (F11): one enum owns which shell surface is active
//! on the `MainMenu` screen.
//!
//! Replaces three mutually-entangled booleans
//! (`main_menu_show_single_player_shell`, `main_menu_show_native_skirmish_shell`,
//! `skirmish_shell_return_to_single_player_shell`) that six hand-written
//! teardown blocks each cleared with slightly different subsets. Exclusivity
//! is now structural: the state can no longer represent two shells at once.
//! A terminal menu error is owned separately by the frontend; it stops shell
//! rendering and input without creating an alternate gameplay route.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ShellRoute {
    /// The stock main menu (or its degraded fallback).
    #[default]
    MainMenu,
    /// The single-player shell page.
    SinglePlayer,
    /// Movies & Credits page `0x101`.
    MoviesAndCredits,
    /// Movie list `0x129`, reached from Movies & Credits.
    MovieList,
    /// Campaign selection `0x94`, reached from Single Player.
    Campaign,
    /// Load Saved Game `0xB7`, reached from Single Player.
    LoadSavedGame,
    /// Westwood Online before the lobby, reached from Internet: welcome
    /// `0x10E`, login `0x10F`, the wait box `0xC4` and the boxes
    /// `0xD0`/`0x126` over the empty backdrop.
    Wol,
    /// The Westwood Online custom-match lobby `0x113`.
    WolLobby,
    /// LAN discovery BB.
    LanLobby,
    /// LAN host BC or guest BD; MatchSession owns the host role.
    NetworkSetup,
    /// The native skirmish shell `0x102`. It is entered only from the
    /// single-player shell (directly, or on resume after a match), so Back
    /// returns there.
    Skirmish,
}

impl ShellRoute {
    pub(crate) fn single_player(self) -> bool {
        matches!(self, Self::SinglePlayer)
    }

    /// Movies & Credits `0x101`.
    pub(crate) fn movies_and_credits(self) -> bool {
        matches!(self, Self::MoviesAndCredits)
    }

    /// Movie list `0x129`.
    pub(crate) fn movie_list(self) -> bool {
        matches!(self, Self::MovieList)
    }

    /// Campaign selection `0x94`.
    pub(crate) fn campaign(self) -> bool {
        matches!(self, Self::Campaign)
    }

    /// Load Saved Game `0xB7`.
    pub(crate) fn load_saved_game(self) -> bool {
        matches!(self, Self::LoadSavedGame)
    }

    /// Westwood Online before the lobby.
    pub(crate) fn wol(self) -> bool {
        matches!(self, Self::Wol)
    }

    pub(crate) fn wol_lobby(self) -> bool {
        matches!(self, Self::WolLobby)
    }

    /// Any Westwood Online surface: one WOL visit owns both routes.
    pub(crate) fn wol_visit(self) -> bool {
        self.wol() || self.wol_lobby()
    }

    pub(crate) fn lan_lobby(self) -> bool {
        matches!(self, Self::LanLobby)
    }
    pub(crate) fn network_setup(self) -> bool {
        matches!(self, Self::NetworkSetup)
    }

    pub(crate) fn skirmish(self) -> bool {
        matches!(self, Self::Skirmish)
    }
}

#[cfg(test)]
mod tests {
    use super::ShellRoute;

    /// F11: shell surfaces are mutually exclusive by construction.
    #[test]
    fn shell_routes_are_exclusive() {
        assert_eq!(ShellRoute::default(), ShellRoute::MainMenu);
        for route in [
            ShellRoute::MainMenu,
            ShellRoute::SinglePlayer,
            ShellRoute::MoviesAndCredits,
            ShellRoute::MovieList,
            ShellRoute::Campaign,
            ShellRoute::LoadSavedGame,
            ShellRoute::Wol,
            ShellRoute::WolLobby,
            ShellRoute::Skirmish,
        ] {
            // At most one surface active — the predicates cannot both hold.
            let active = [
                route.single_player(),
                route.movies_and_credits(),
                route.movie_list(),
                route.campaign(),
                route.load_saved_game(),
                route.wol(),
                route.wol_lobby(),
                route.skirmish(),
            ];
            assert!(active.iter().filter(|&&on| on).count() <= 1);
        }
    }
}
