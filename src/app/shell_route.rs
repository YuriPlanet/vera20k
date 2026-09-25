//! Frontend shell route (F11): one enum owns which shell surface is active
//! on the `MainMenu` screen.
//!
//! Replaces three mutually-entangled booleans
//! (`main_menu_show_single_player_shell`, `main_menu_show_native_skirmish_shell`,
//! `skirmish_shell_return_to_single_player_shell`) that six hand-written
//! teardown blocks each cleared with slightly different subsets. Exclusivity
//! is now structural: the state can no longer represent two shells at once,
//! and the skirmish return arrow travels inside the variant it belongs to.
//! The env-gated `dev_skirmish_shell_enabled` override and the degraded
//! `main_menu_shell_failed` latch remain separate flags — they gate rendering,
//! not routing.

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
    /// The native skirmish shell. `return_to_single_player` is the return
    /// arrow: entered from the single-player shell, Back returns there
    /// instead of the main menu.
    Skirmish { return_to_single_player: bool },
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

    pub(crate) fn skirmish(self) -> bool {
        matches!(self, Self::Skirmish { .. })
    }

    pub(crate) fn skirmish_returns_to_single_player(self) -> bool {
        matches!(
            self,
            Self::Skirmish {
                return_to_single_player: true
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::ShellRoute;

    /// F11: shell surfaces are mutually exclusive by construction, and the
    /// return arrow only exists inside the skirmish variant.
    #[test]
    fn shell_routes_are_exclusive_and_carry_the_return_arrow() {
        assert_eq!(ShellRoute::default(), ShellRoute::MainMenu);
        for route in [
            ShellRoute::MainMenu,
            ShellRoute::SinglePlayer,
            ShellRoute::MoviesAndCredits,
            ShellRoute::MovieList,
            ShellRoute::Campaign,
            ShellRoute::Skirmish {
                return_to_single_player: false,
            },
            ShellRoute::Skirmish {
                return_to_single_player: true,
            },
        ] {
            // At most one surface active — the predicates cannot both hold.
            let active = [
                route.single_player(),
                route.movies_and_credits(),
                route.movie_list(),
                route.campaign(),
                route.skirmish(),
            ];
            assert!(active.iter().filter(|&&on| on).count() <= 1);
        }
        assert!(
            ShellRoute::Skirmish {
                return_to_single_player: true
            }
            .skirmish_returns_to_single_player()
        );
        assert!(
            !ShellRoute::Skirmish {
                return_to_single_player: false
            }
            .skirmish_returns_to_single_player()
        );
        assert!(!ShellRoute::SinglePlayer.skirmish_returns_to_single_player());
    }
}
