//! Frontend shell route: one enum owns which shell surface is active on the
//! `MainMenu` screen.
//!
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
    /// VERA's Mods page (`ui::mods_shell`), reached from the main menu.
    Mods,
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
