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

/// ShellMessageBox__Run `0x005D3490`'s boxes, read from CSF once at open:
/// body, then the buttons that answer 0 (OK slot), 1 (Cancel) and 2 (third).
fn message(csf: &CsfLookup<'_>, [body, ok, cancel, third]: [(&str, &str); 4]) -> ShellBox {
    let text = |(key, fallback): (&str, &str)| match key {
        "" => String::new(),
        key => csf(key, fallback),
    };
    ShellBox::message(text(body), text(ok), text(cancel), text(third))
}

const NONE: (&str, &str) = ("", "");
const OK: (&str, &str) = ("TXT_OK", "OK");
const CANCEL: (&str, &str) = ("GUI:Cancel", "Cancel");

/// MapSelect's failed Init (FUN_005ADD10 @0x005ADDA6..5ADDCF): only TXT_OK.
pub(crate) fn map_select_failure(csf: &CsfLookup<'_>) -> ShellBox {
    let body = ("MSG:UnableMapSelect", "Unable to initiate Map Selection!");
    message(csf, [body, OK, NONE, NONE])
}

/// Main-menu Exit Game (state 6): OK quits, Cancel stays.
pub(crate) fn exit_confirm(csf: &CsfLookup<'_>) -> ShellBox {
    let body = ("GUI:ExitAreYouSure", "Are you sure you want to quit?");
    message(csf, [body, OK, CANCEL, NONE])
}

/// VERA's Esc menu: the main menu's own Options (0), Cancel (1) and Exit
/// Game (2), and no question, its buttons in a footer.
pub(crate) fn shell_menu(csf: &CsfLookup<'_>) -> ShellBox {
    let (options, exit) = (("GUI:Options", "Options"), ("GUI:ExitGame", "Exit Game"));
    message(csf, [NONE, options, CANCEL, exit]).template(crate::ui::shell::modal::FOOTER_TEMPLATE)
}

/// GameExit::Defeat's campaign retry question: Yes (0) and No (1), loaded at
/// `0x006860A0..0x006860DB` (strings 0x0083DB24, 0x00829F14, 0x00829F1C).
pub(crate) fn campaign_replay(csf: &CsfLookup<'_>) -> ShellBox {
    let body = ("TXT_TO_REPLAY", "Would you like to replay this mission?");
    message(csf, [body, ("TXT_YES", "Yes"), ("TXT_NO", "No"), NONE])
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;

    /// The captions land in answer order: the Esc menu's Options answers
    /// 0, Cancel 1 and Exit Game 2.
    #[test]
    fn message_boxes_read_their_csf_keys_in_answer_order() {
        let lookup = |key: &str, _fallback: &str| key.to_string();
        let captions = |b: ShellBox| -> Vec<String> {
            let mut texts = vec![b.body];
            texts.extend(b.buttons.into_iter().map(|b| b.caption));
            texts
        };
        assert_eq!(
            captions(exit_confirm(&lookup)),
            ["GUI:ExitAreYouSure", "TXT_OK", "GUI:Cancel"]
        );
        assert_eq!(
            captions(campaign_replay(&lookup)),
            ["TXT_TO_REPLAY", "TXT_YES", "TXT_NO"]
        );
        assert_eq!(
            captions(map_select_failure(&lookup)),
            ["MSG:UnableMapSelect", "TXT_OK"]
        );
        assert_eq!(
            captions(shell_menu(&lookup)),
            ["", "GUI:Options", "GUI:Cancel", "GUI:ExitGame"]
        );
    }
}
