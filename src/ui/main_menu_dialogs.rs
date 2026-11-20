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

use crate::ui::shell::modal::ShellBox;

/// Resolves a CSF string key to display text, with an English fallback when the
/// table is missing the key. Provided by the caller (which owns the CSF table).
pub(crate) type CsfLookup<'a> = dyn Fn(&str, &str) -> String + 'a;

// ---------------------------------------------------------------------------
// Two-button shell message box
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

/// GameExit::Defeat's campaign retry question and buttons, loaded at
/// `0x006860A0..0x006860DB` (strings 0x0083DB24, 0x00829F14, 0x00829F1C).
pub(crate) const CAMPAIGN_REPLAY_TITLE_KEY: &str = "TXT_TO_REPLAY";
pub(crate) const CAMPAIGN_REPLAY_YES_KEY: &str = "TXT_YES";
pub(crate) const CAMPAIGN_REPLAY_NO_KEY: &str = "TXT_NO";

/// The Esc menu's buttons, captioned as the main menu's own.
const SHELL_MENU_OPTIONS_KEY: &str = "GUI:Options";
const SHELL_MENU_EXIT_KEY: &str = "GUI:ExitGame";

/// ShellMessageBox__Run `0x005D3490`'s boxes, read from CSF once at open:
/// body, the OK-slot button (`0x5AE`, result 0), the Cancel slot (control 2,
/// result 1) and the third (result 2); the count rule picks CE/120/121.
pub(crate) fn map_select_failure(csf: &CsfLookup<'_>) -> ShellBox {
    // FUN_005ADD10 @0x005ADDA6..5ADDCF: only TXT_OK is supplied.
    let body = csf("MSG:UnableMapSelect", "Unable to initiate Map Selection!");
    ShellBox::notice(body, csf("TXT_OK", "OK"))
}

/// Main-menu Exit Game (state 6).
pub(crate) fn exit_confirm(csf: &CsfLookup<'_>) -> ShellBox {
    ShellBox::message(
        csf(EXIT_CONFIRM_TITLE_KEY, EXIT_CONFIRM_TITLE_FALLBACK),
        csf(EXIT_CONFIRM_OK_KEY, EXIT_CONFIRM_OK_FALLBACK),
        csf(EXIT_CONFIRM_CANCEL_KEY, EXIT_CONFIRM_CANCEL_FALLBACK),
        String::new(),
    )
}

/// VERA's Esc menu: the main menu's own Options and Exit Game captions in
/// the OK and third slots, and no question, its buttons in a footer.
pub(crate) fn shell_menu(csf: &CsfLookup<'_>) -> ShellBox {
    ShellBox::message(
        String::new(),
        csf(SHELL_MENU_OPTIONS_KEY, "Options"),
        csf(EXIT_CONFIRM_CANCEL_KEY, EXIT_CONFIRM_CANCEL_FALLBACK),
        csf(SHELL_MENU_EXIT_KEY, "Exit Game"),
    )
    .form(crate::ui::shell::modal::BoxForm::Footer)
}

/// Campaign defeat's retry prompt: Yes in the OK slot, No in Cancel's.
pub(crate) fn campaign_replay(csf: &CsfLookup<'_>) -> ShellBox {
    ShellBox::message(
        csf(
            CAMPAIGN_REPLAY_TITLE_KEY,
            "Would you like to replay this mission?",
        ),
        csf(CAMPAIGN_REPLAY_YES_KEY, "Yes"),
        csf(CAMPAIGN_REPLAY_NO_KEY, "No"),
        String::new(),
    )
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::ui::shell::modal::control::{CANCEL, OK, THIRD};

    #[test]
    fn message_boxes_read_their_csf_keys() {
        // A lookup that returns the key itself proves which keys are queried.
        let lookup = |key: &str, _fallback: &str| key.to_string();
        let modal = exit_confirm(&lookup);
        assert_eq!(modal.body, EXIT_CONFIRM_TITLE_KEY);
        assert_eq!(modal.caption(OK), Some(EXIT_CONFIRM_OK_KEY));
        assert_eq!(modal.caption(CANCEL), Some(EXIT_CONFIRM_CANCEL_KEY));
        let replay = campaign_replay(&lookup);
        assert_eq!(replay.body, CAMPAIGN_REPLAY_TITLE_KEY);
        assert_eq!(replay.caption(OK), Some(CAMPAIGN_REPLAY_YES_KEY));
        assert_eq!(replay.caption(CANCEL), Some(CAMPAIGN_REPLAY_NO_KEY));
        assert_eq!(replay.kind(), crate::ui::shell::modal::ModalKind::Confirm);
        let failed = map_select_failure(&lookup);
        assert_eq!(failed.body, "MSG:UnableMapSelect");
        assert_eq!(failed.caption(OK), Some("TXT_OK"));
        assert!(failed.caption(CANCEL).is_none());
        assert_eq!(failed.kind(), crate::ui::shell::modal::ModalKind::BodyOk);
    }

    /// Options answers 0, Exit Game 2 and Cancel 1, so the three captions
    /// must land in the OK, third and Cancel slots of the three-button box.
    #[test]
    fn the_esc_menu_is_the_three_button_box() {
        let lookup = |key: &str, _fallback: &str| key.to_string();
        let menu = shell_menu(&lookup);
        assert_eq!(menu.caption(OK), Some(SHELL_MENU_OPTIONS_KEY));
        assert_eq!(menu.caption(THIRD), Some(SHELL_MENU_EXIT_KEY));
        assert_eq!(menu.caption(CANCEL), Some(EXIT_CONFIRM_CANCEL_KEY));
        assert_eq!(menu.kind(), crate::ui::shell::modal::ModalKind::ThreeButton);
    }
}
