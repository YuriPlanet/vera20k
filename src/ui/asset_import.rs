//! The import dialog shown when the game installation is missing.
//!
//! This is the one dialog that has to draw without retail art — the situation
//! it exists for — so it owns no shell artwork at all. It carries the words, the
//! step it is on and the button labels; `app::frontend::status_screen` renders
//! it from the in-repo face, the same card that already reports a missing
//! installation.
//!
//! The folder picker itself is the platform's business: Android has to reach
//! `ACTION_OPEN_DOCUMENT_TREE` through JNI, the desktop builds already have a
//! native chooser. [`choose_folder`] is the seam; until a platform fills it in,
//! the dialog explains and the button reports that picking is unavailable.
//!
//! Every string here is ASCII: the card's in-repo face carries ASCII only, and
//! the exact paths live in the log.

use std::path::PathBuf;

/// Where the import flow stands.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) enum ImportStep {
    /// The card is not showing the import dialog.
    #[default]
    Closed,
    /// Explaining what to pick, before the player confirms.
    ChooseFolder,
    /// Copying the installation into the app's own folder.
    Copying { done: u64, total: Option<u64> },
    /// Copied; the engine needs a restart to read the new folder.
    Done,
    /// The last attempt failed; the reason is what the card shows.
    Failed(String),
}

/// The import dialog's state.
#[derive(Debug, Default)]
pub(crate) struct AssetImport {
    step: ImportStep,
}

impl AssetImport {
    pub(crate) fn step(&self) -> &ImportStep {
        &self.step
    }

    /// Draw and take input for the dialog rather than the plain error card.
    pub(crate) fn is_open(&self) -> bool {
        self.step != ImportStep::Closed
    }

    /// The player asked to import: switch to explaining what to pick.
    pub(crate) fn open(&mut self) {
        if self.step == ImportStep::Closed {
            self.step = ImportStep::ChooseFolder;
        }
    }

    /// Back to the plain card, leaving the error text underneath untouched.
    pub(crate) fn close(&mut self) {
        self.step = ImportStep::Closed;
    }

    /// The platform handed a copy job its size (unknown for some sources,
    /// where `content://` reports no length).
    pub(crate) fn start_copy(&mut self, total: Option<u64>) {
        self.step = ImportStep::Copying { done: 0, total };
    }

    /// Bytes copied so far, so the card can show how far along it is.
    pub(crate) fn advance_copy(&mut self, done: u64) {
        if let ImportStep::Copying { total, .. } = self.step {
            self.step = ImportStep::Copying { done, total };
        }
    }

    pub(crate) fn finish_copy(&mut self) {
        self.step = ImportStep::Done;
    }

    pub(crate) fn fail(&mut self, why: impl Into<String>) {
        self.step = ImportStep::Failed(why.into());
    }

    /// Completion as a fraction, when the source reported a size.
    pub(crate) fn progress(&self) -> Option<f32> {
        match &self.step {
            ImportStep::Copying {
                done,
                total: Some(total),
            } if *total > 0 => Some((*done as f32 / *total as f32).clamp(0.0, 1.0)),
            _ => None,
        }
    }

    pub(crate) fn title(&self) -> &'static str {
        "Import Game Files"
    }

    /// The card's body: what is missing, what to pick, and the current step.
    pub(crate) fn body(&self, game_files: &str, config: &str) -> String {
        let mut text = String::from(
            "VERA20K needs Red Alert 2: Yuri's Revenge to run.\n\n\
Choose the folder that holds ra2md.mix and ra2.mix.\n\
VERA20K copies it once into its own folder and plays from that copy, so the \
original installation is never modified.\n\n\
About 1.5 GB is needed.\n\n",
        );
        text.push_str(&match &self.step {
            ImportStep::Closed | ImportStep::ChooseFolder => String::new(),
            ImportStep::Copying { done, total } => match total {
                Some(total) => format!(
                    "Copying: {} of {} MB.\n\n",
                    done / (1024 * 1024),
                    total / (1024 * 1024)
                ),
                None => format!("Copying: {} MB.\n\n", done / (1024 * 1024)),
            },
            ImportStep::Done => "Copied. Restart VERA20K to play.\n\n".to_owned(),
            ImportStep::Failed(why) => format!("Failed: {why}\n\n"),
        });
        text.push_str(&format!("Game files: {game_files}"));
        if !config.is_empty() {
            text.push_str(&format!("\nConfig: {config}"));
        }
        text
    }

    /// Button labels for the card, left slot first. The right slot is `None`
    /// while a copy is running, so the only way out is to let it finish.
    pub(crate) fn buttons(&self) -> [Option<&'static str>; 2] {
        match &self.step {
            ImportStep::Closed => [Some("Import Game Files"), None],
            ImportStep::ChooseFolder | ImportStep::Failed(_) => {
                [Some("Choose Folder"), Some("Cancel")]
            }
            ImportStep::Copying { .. } => [None, Some("Cancel")],
            ImportStep::Done => [Some("Close"), None],
        }
    }

    /// Which button was pressed, by the card's left/right slot.
    pub(crate) fn press_left(&mut self) -> ButtonPress {
        match &self.step {
            ImportStep::Closed => {
                self.open();
                ButtonPress::PickFolder
            }
            ImportStep::ChooseFolder | ImportStep::Failed(_) => ButtonPress::PickFolder,
            ImportStep::Copying { .. } => ButtonPress::Ignored,
            ImportStep::Done => {
                self.close();
                ButtonPress::Ignored
            }
        }
    }

    pub(crate) fn press_right(&mut self) -> ButtonPress {
        if matches!(
            self.step,
            ImportStep::ChooseFolder | ImportStep::Failed(_) | ImportStep::Copying { .. }
        ) {
            self.close();
        }
        ButtonPress::Ignored
    }
}

/// What the pressed button asks the caller to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ButtonPress {
    /// Open the platform's folder picker.
    PickFolder,
    /// Handled inside the dialog.
    Ignored,
}

/// Ask the platform for the folder holding the retail installation.
///
/// `None` means this build has no picker yet: Android still has to reach SAF
/// through JNI, and the dialog then says so rather than pretending to import.
pub(crate) fn choose_folder() -> Option<PathBuf> {
    log::info!("asset import: this build has no folder picker wired yet");
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_moves_from_the_plain_card_to_the_explanation() {
        let mut dialog = AssetImport::default();
        assert!(!dialog.is_open());
        assert_eq!(dialog.buttons(), [Some("Import Game Files"), None]);
        assert_eq!(dialog.press_left(), ButtonPress::PickFolder);
        assert!(dialog.is_open());
        assert_eq!(dialog.buttons(), [Some("Choose Folder"), Some("Cancel")]);
    }

    #[test]
    fn cancelling_returns_to_the_plain_card() {
        let mut dialog = AssetImport::default();
        dialog.open();
        dialog.press_right();
        assert!(!dialog.is_open());
    }

    #[test]
    fn a_copy_without_a_known_size_still_reports_progress_lines() {
        let mut dialog = AssetImport::default();
        dialog.open();
        dialog.start_copy(None);
        dialog.advance_copy(8 * 1024 * 1024);
        assert_eq!(dialog.progress(), None);
        assert!(dialog.body("", "").contains("Copying: 8 MB."));
        assert_eq!(dialog.buttons(), [None, Some("Cancel")]);
    }

    #[test]
    fn a_copy_with_a_known_size_reports_a_fraction() {
        let mut dialog = AssetImport::default();
        dialog.start_copy(Some(100));
        dialog.advance_copy(25);
        assert_eq!(dialog.progress(), Some(0.25));
    }

    #[test]
    fn a_failure_offers_another_attempt() {
        let mut dialog = AssetImport::default();
        dialog.open();
        dialog.start_copy(Some(10));
        dialog.fail("not enough space");
        assert!(dialog.body("", "").contains("Copy failed: not enough space"));
        assert_eq!(dialog.buttons(), [Some("Choose Folder"), Some("Cancel")]);
        assert_eq!(dialog.press_left(), ButtonPress::PickFolder);
    }
}
