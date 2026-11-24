//! Game screen state machine — which UI screen is active.
//!
//! The App checks this each frame to decide what to render (menu vs game).
//! Transitions are triggered by UI actions (e.g., clicking "Quick Play")
//! or loading completion.
//!
//! ## Dependency rules
//! - Part of ui/ — no dependencies on render/, assets/, sim/, etc.
//! - Pure data enum. App.rs reads and mutates this.

/// Which screen the application is currently displaying.
///
/// Transitions:
/// - MainMenu → Loading (user clicks "Start Game")
/// - Loading → InGame (the loaded scenario starts)
/// - Loading → MissionResult (the load fails or its startup is rejected)
/// - InGame → MainMenu (user presses Escape)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum GameScreen {
    /// The retail menu shell. No map is loaded yet.
    #[default]
    MainMenu,

    /// Transitional state: map is being loaded.
    ///
    /// The active `LoadingSession` owns the selected map and launch data.
    Loading,

    /// In-game: terrain, units, sprites are rendered.
    /// The retail sidebar and in-scenario shell share the game state owners.
    InGame,

    /// Mission ended: victory, defeat, or script-forced end state.
    MissionResult { title: String, detail: String },
}
