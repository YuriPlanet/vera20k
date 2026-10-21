//! Retail in-scenario modal state and transitions for the in-game menu and
//! abort-mission confirmation. Physical dialog presentation lives in the app
//! shell compositor and `ui::shell` resource geometry.
//!
//! ## What gamemd does
//!
//! A scenario carries one in-scenario state variable. Zero means "no modal, the
//! world runs"; every other value names a modal the scenario state machine
//! dispatches to. The states reachable in a stock offline skirmish are:
//!
//! | state | dialog | how it is entered | where it goes |
//! |---|---|---|---|
//! | 0 | none | every other state falls back here | the mission runs |
//! | 1 | the in-game menu | the sidebar menu control | its buttons |
//! | 3 | abort-mission confirm | the menu's Abort Mission button | see below |
//! | 5 | Game Controls / Options | the menu's Options button | back to 1 |
//! | 8 | Diplomacy (dialog 0x73) | the sidebar Diplomacy button, Tab | Return: 0 |
//! | 9 | mission restate briefing (dialog 0x10C) | the sidebar Diplomacy button in campaign and Cooperative | Return: 0 |
//!
//! Two re-entry rules matter to the player and are reproduced here:
//!
//! * **Options is a child of the menu.** When the Options dialog returns, the
//!   state machine re-enters state 1 — closing Options puts you back on the
//!   menu, it does not resume the mission.
//! * **Cancelling the abort confirmation resumes the mission**, it does not
//!   return to the menu: the confirm dialog's cancel result matches no case in
//!   the state machine's switch, so it falls through to state 0.
//!
//! The in-game menu's own dialog template is selected by game mode; offline
//! campaign and skirmish share one template whose controls are Load Game, Save
//! Game, Delete Game, Game Controls, Abort Mission and Resume Mission. B5 at
//! BEF9D0 contains exactly these six buttons and two statics; no mission-restate
//! control. Its three saved-game children share the native558DD0 browser owner.
//!
//! The full-screen B6 abort shell has a secondary action that is mode
//! dependent: campaign labels it `GUI:Restart` (restart the scenario),
//! multiplayer with two or more live human players labels it `GUI:Observe`
//! (EVENT_DESTRUCT for the local House, then keep watching; Leave there also
//! destructs before EXIT), and **offline skirmish hides it outright**. What is left in skirmish is one
//! action button captioned `GUI:Leave` and Resume Mission. Callback4F18B0
//! accepts Resume686 and IDOK1; IDCANCEL2 is ignored, so Escape stays in B6.
//! Confirming Leave queues an
//! EXIT event for the local player; when that event executes it raises the
//! graceful-exit session flag, which tears the session down *without* the
//! victory or defeat teardown — no result screen, no outcome announcement.
//!
//! Evidence: live decompilation of gamemd.exe this session — the scenario state
//! machine, the in-game menu modal and its dialog proc, the abort-confirm modal
//! and its dialog proc, the exit-event helper, `EventClass::Execute`'s EXIT
//! case, and the session-end router.
//!
//! ## Dependency rules
//! - Part of ui/ — no dependencies on render/, assets/, sim/, audio/.
//! - The state enum is pure data with pure transitions; the app layer owns the
//!   side effects (freezing the sim, tearing the match down).

/// In-scenario modal state. Mirrors gamemd's in-scenario state variable,
/// restricted to the values a stock offline skirmish can reach.
///
/// States deliberately absent: the surrender-and-be-scored state (gated on a
/// WOL-only mode), the replay state, the multiplayer objectives state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InGameMenuState {
    /// No modal is open — the mission runs. (gamemd: 0)
    #[default]
    Closed,
    /// The in-game menu. (gamemd: 1)
    Menu,
    /// The abort-mission confirmation. (gamemd: 3)
    AbortConfirm,
    /// Game Controls / Options — a child of [`InGameMenuState::Menu`]. (gamemd: 5)
    Options,
    /// Sound options, active B8 (native state6).
    Sound,
    /// Shared Keyboard A3, child of Game Controls (native state4).
    Keyboard,
    /// The WOL page dialog 0xCF, opened by PageUser. (gamemd: 7) It is a
    /// compact popup over the live battlefield, not a full-screen shell.
    Page,
    /// The Diplomacy dialog 0x73. (gamemd: 8)
    Diplomacy,
    /// The mission restate briefing, dialog 0x10C. (gamemd: 9)
    MissionBriefing,
    /// Native558DD0 browser nested beneath B5. Its mode has no independent
    /// scenario-state number: native keeps the parent hidden during this call.
    SavedGame(crate::ui::skirmish_shell::SavedSeedMode),
}

impl InGameMenuState {
    /// True while any in-scenario modal owns the screen. The app layer freezes
    /// the simulation for exactly this condition.
    pub fn is_open(self) -> bool {
        !matches!(self, Self::Closed)
    }

    /// The battlefield keeps drawing under this modal: the modal pump's
    /// Main_Tick still blits the tactical surface (`0x004F4780` only changes
    /// its copy flag while `g_WSDialogCount` is nonzero), and the popup
    /// window sits over it.
    pub fn draws_over_battlefield(self) -> bool {
        matches!(self, Self::Page)
    }

    /// Where Escape goes from here.
    ///
    /// Escape opens Options through5372D0→647040→4C7939. B5 itself ignores
    /// IDCANCEL2 at4F1320..4F134E and leaves its result unchanged at4F16F1.
    /// Modeless622650→5D4D50 supplies IsDialogMessageA; Windows sends IDCANCEL
    /// but requires the application callback to dismiss the dialog.
    /// See docs/research/skirmish-ui/2026-09-12-pause-save-shells-evidence.md.
    pub fn on_escape(self) -> Self {
        match self {
            // Nothing open: Escape stands in for the sidebar menu control.
            Self::Closed => Self::Menu,
            // B5 has no cancel command; Resume is an explicit button action.
            Self::Menu => Self::Menu,
            // B6 callback4F1A37..4F1A97 ignores IDCANCEL2. Its explicit
            // Resume686 and default IDOK1 resume instead.
            Self::AbortConfirm => Self::AbortConfirm,
            // Options is a child of the menu.
            Self::Options => Self::Menu,
            // The 0x73 proc resolves its command by GetDlgCtrlID(lParam)
            // (0x006581A7). IDCANCEL names no child window, so the id is 0:
            // neither Return 0x423 nor in its box tables. Escape is ignored.
            // The 0x10C proc keys Return the same way (0x0065F710).
            // The 0xCF loop (0x007759E0 with 0x00623120) runs no
            // IsDialogMessage, so Escape never reaches the proc.
            Self::SavedGame(_)
            | Self::Sound
            | Self::Diplomacy
            | Self::MissionBriefing
            | Self::Page => self,
            Self::Keyboard => Self::Options,
        }
    }
}

/// Does an Escape press belong to the in-scenario modal machine?
///
/// Existing dialogs own Escape. Ordinary world input must first resolve the
/// reloaded binding; Options is only ProcessCommand55E079's fallback.
pub fn escape_belongs_to_modal_machine(state: InGameMenuState) -> bool {
    state.is_open()
}

/// What the player picked on the abort-mission confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbortConfirmAction {
    /// Nothing this frame.
    None,
    /// Cancel — resume the mission.
    Cancel,
    /// Leave — end the session through the graceful-exit route.
    Leave,
    /// Campaign Restart — B6 result 5 (`0x004F196B`).
    Restart,
    /// LAN/WOL Leave while Observe is offered — B6 result 3.
    LeaveDestruct,
    /// LAN/WOL Observe — B6 result 4.
    Observe,
}

/// What the app layer must do once a modal reports the player's choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalOutcome {
    /// Nothing was picked this frame — the modal stays as it is.
    Stay,
    /// Move the in-scenario state variable to this value.
    Enter(InGameMenuState),
    /// End the running match by the graceful-exit route. `destruct` first
    /// queues EVENT_DESTRUCT for the local House: the state machine's
    /// result-3 arm (`0x0048CA34..0x0048CAA8`) queues it and falls into the
    /// result-2 EXIT (`0x6471A0`).
    LeaveMatch { destruct: bool },
    /// Queue EVENT_DESTRUCT for the local House and keep watching: the
    /// result-4 arm (`0x0048CAB2`) clears the admission latch without EXIT.
    Observe,
    /// Raise the scenario restart request: the state machine's result-5 arm
    /// (`0x0048CB2E`) sets `0x008B41C0` and the modal state returns to 0.
    RestartScenario,
}

/// The abort confirmation's button routes.
///
/// Cancel resumes the mission — it does **not** return to the menu, because
/// gamemd's cancel result matches no case in the state machine's switch and
/// falls through to state 0. Confirming leaves the match.
pub fn resolve_abort_action(action: AbortConfirmAction) -> ModalOutcome {
    match action {
        AbortConfirmAction::None => ModalOutcome::Stay,
        AbortConfirmAction::Cancel => ModalOutcome::Enter(InGameMenuState::Closed),
        AbortConfirmAction::Leave => ModalOutcome::LeaveMatch { destruct: false },
        AbortConfirmAction::LeaveDestruct => ModalOutcome::LeaveMatch { destruct: true },
        AbortConfirmAction::Restart => ModalOutcome::RestartScenario,
        AbortConfirmAction::Observe => ModalOutcome::Observe,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mission runs only at state 0; every modal freezes it.
    #[test]
    fn only_the_closed_state_lets_the_mission_run() {
        assert!(!InGameMenuState::Closed.is_open());
        assert!(InGameMenuState::Menu.is_open());
        assert!(InGameMenuState::AbortConfirm.is_open());
        assert!(InGameMenuState::Options.is_open());
        assert!(InGameMenuState::Diplomacy.is_open());
        assert!(InGameMenuState::MissionBriefing.is_open());
    }

    /// Dialogs 0x73 and 0x10C have no cancel path: only Return closes them.
    #[test]
    fn escape_keeps_diplomacy_and_the_briefing_open() {
        for state in [InGameMenuState::Diplomacy, InGameMenuState::MissionBriefing] {
            assert_eq!(state.on_escape(), state);
            assert!(escape_belongs_to_modal_machine(state));
        }
    }

    /// Doing nothing in the abort dialog leaves the modal where it was.
    #[test]
    fn no_choice_leaves_the_modal_alone() {
        assert_eq!(
            resolve_abort_action(AbortConfirmAction::None),
            ModalOutcome::Stay
        );
    }

    /// Options is a child of the menu: Game Controls enters it, and both ways
    /// out of it — its own Back control and Escape — return to the menu (5 → 1)
    /// rather than resuming the mission.
    #[test]
    fn options_returns_to_the_menu_not_to_the_mission() {
        assert_eq!(InGameMenuState::Options.on_escape(), InGameMenuState::Menu);
    }

    /// Explicit Resume ends at state 0, while native IDCANCEL is ignored.
    #[test]
    fn abort_cancel_resumes_the_mission_instead_of_returning_to_the_menu() {
        assert_eq!(
            resolve_abort_action(AbortConfirmAction::Cancel),
            ModalOutcome::Enter(InGameMenuState::Closed)
        );
        assert_eq!(
            InGameMenuState::AbortConfirm.on_escape(),
            InGameMenuState::AbortConfirm
        );
    }

    /// Confirming abort leaves the match rather than moving the modal state.
    #[test]
    fn confirm_abort_leaves_the_match() {
        assert_eq!(
            resolve_abort_action(AbortConfirmAction::Leave),
            ModalOutcome::LeaveMatch { destruct: false }
        );
    }

    /// Network Leave with Observe offered destructs before EXIT; Observe
    /// only destructs.
    #[test]
    fn network_leave_and_observe_destruct_the_local_house() {
        assert_eq!(
            resolve_abort_action(AbortConfirmAction::LeaveDestruct),
            ModalOutcome::LeaveMatch { destruct: true }
        );
        assert_eq!(
            resolve_abort_action(AbortConfirmAction::Observe),
            ModalOutcome::Observe
        );
    }

    /// Campaign Restart raises the restart request instead of leaving.
    #[test]
    fn campaign_restart_requests_a_scenario_restart() {
        assert_eq!(
            resolve_abort_action(AbortConfirmAction::Restart),
            ModalOutcome::RestartScenario
        );
    }

    /// Escape opens B5, but B5 ignores IDCANCEL and stays paused.
    #[test]
    fn escape_opens_and_closes_the_menu() {
        assert_eq!(InGameMenuState::Closed.on_escape(), InGameMenuState::Menu);
        assert_eq!(InGameMenuState::Menu.on_escape(), InGameMenuState::Menu);
    }

    /// The handler must not intercept world Escape before the command lookup.
    #[test]
    fn closed_world_resolves_escape_bindings_before_opening_the_menu() {
        assert!(!escape_belongs_to_modal_machine(InGameMenuState::Closed,));
        for open in [
            InGameMenuState::Menu,
            InGameMenuState::AbortConfirm,
            InGameMenuState::Options,
        ] {
            assert!(escape_belongs_to_modal_machine(open));
        }
    }
}
