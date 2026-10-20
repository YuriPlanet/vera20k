//! Main-menu modal dialogs reachable from the native main-menu shell.
//!
//! The native shell (`ui::main_menu_shell`) emits owner-draw button actions on
//! mouse-up. Two of those actions open modal dialogs that the original game
//! pops on top of the menu rather than acting immediately (Movies & Credits
//! is the native page in `ui::movies_credits_shell`, campaign selection the
//! native dialog in `ui::campaign_shell`):
//!
//! - Exit Game -> a confirm message box ("are you sure?") with confirm/cancel.
//!   The game does NOT quit on the first click; it quits only on confirm.
//! - Options -> the retained launcher Options parent in `options`.
//!
//! Exit confirmation and launcher Options render through the retail shell.
//! State lives on `AppState` as `Option<...>` fields and persists across frames
//! while open. All button labels resolve from the live
//! CSF table (passed in by the caller) with English fallbacks; no CSF text is
//! hardcoded as the source of truth.
//!
//! ## Dependency rules
//! - app/UI layer only; never referenced from `sim/`.

pub(crate) mod options;

pub(crate) use options::OptionsDialogState;

/// Resolves a CSF string key to display text, with an English fallback when the
/// table is missing the key. Provided by the caller (which owns the CSF table).
pub(crate) type CsfLookup<'a> = dyn Fn(&str, &str) -> String + 'a;

// ---------------------------------------------------------------------------
// Exit confirm message box
// ---------------------------------------------------------------------------

/// CSF key for the confirm-dialog body/title text.
pub(crate) const EXIT_CONFIRM_TITLE_KEY: &str = "GUI:ExitAreYouSure";
/// CSF key for the confirm (quit) button. Return 0 in the original = confirm.
pub(crate) const EXIT_CONFIRM_OK_KEY: &str = "TXT_OK";
/// CSF key for the cancel (stay) button. Non-zero in the original = stay.
pub(crate) const EXIT_CONFIRM_CANCEL_KEY: &str = "GUI:Cancel";

const EXIT_CONFIRM_TITLE_FALLBACK: &str = "Are you sure you want to quit?";
const EXIT_CONFIRM_OK_FALLBACK: &str = "OK";
const EXIT_CONFIRM_CANCEL_FALLBACK: &str = "Cancel";

/// State for the Exit-Game confirm message box. Holds resolved strings so the
/// CSF table is read once at open, not every frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExitConfirmModalState {
    pub title: String,
    pub confirm: String,
    pub cancel: String,
}

impl ExitConfirmModalState {
    /// Open the modal, resolving labels through the CSF lookup.
    pub fn open(csf: &CsfLookup<'_>) -> Self {
        Self {
            title: csf(EXIT_CONFIRM_TITLE_KEY, EXIT_CONFIRM_TITLE_FALLBACK),
            confirm: csf(EXIT_CONFIRM_OK_KEY, EXIT_CONFIRM_OK_FALLBACK),
            cancel: csf(EXIT_CONFIRM_CANCEL_KEY, EXIT_CONFIRM_CANCEL_FALLBACK),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_confirm_open_uses_csf_when_present() {
        // A lookup that returns the key itself proves open() queries the keys.
        let lookup = |key: &str, _fallback: &str| key.to_string();
        let modal = ExitConfirmModalState::open(&lookup);
        assert_eq!(modal.title, EXIT_CONFIRM_TITLE_KEY);
        assert_eq!(modal.confirm, EXIT_CONFIRM_OK_KEY);
        assert_eq!(modal.cancel, EXIT_CONFIRM_CANCEL_KEY);
    }
}
