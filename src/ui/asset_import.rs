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

/// Whether this build offers the import wizard.
///
/// Android only. The desktop builds keep the startup card exactly as it was
/// before this dialog existed — a single Quit — because a packaged desktop
/// launch already has a native folder chooser. Every gate on this dialog (the
/// card's button, the body swap) uses this constant, so the desktop card cannot
/// drift one branch at a time.
pub(crate) const AVAILABLE: bool = cfg!(target_os = "android");

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
    /// Waiting for the copying worker to stop and remove its partial files.
    Cancelling,
    /// Copied; the engine needs a restart to read the new folder.
    Done,
    /// The last attempt failed; the reason is what the card shows.
    Failed(String),
}

/// The import dialog's state.
#[derive(Debug, Default)]
pub(crate) struct AssetImport {
    step: ImportStep,
    /// Where the finished copy landed. The running process still holds the
    /// config it read at start-up, so this is the only way the card can name the
    /// folder the next launch will use.
    imported: Option<String>,
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

    pub(crate) fn cancel_copy(&mut self) {
        self.step = ImportStep::Cancelling;
    }

    /// Bytes copied so far, so the card can show how far along it is.
    pub(crate) fn advance_copy(&mut self, done: u64) {
        if let ImportStep::Copying { total, .. } = self.step {
            self.step = ImportStep::Copying { done, total };
        }
    }

    /// The copy finished, at [path]: the card names that folder from here on,
    /// because the running process still holds the config it read at start-up.
    pub(crate) fn finish_copy(&mut self, path: impl Into<String>) {
        self.imported = Some(path.into());
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
            ImportStep::Cancelling => "Cancelling. Please wait.\n\n".to_owned(),
            ImportStep::Failed(why) => format!("Failed: {why}\n\n"),
        });
        // Once the copy exists, name it: the caller's `game_files` still holds
        // the folder this process started with, which is exactly the stale value
        // the copy just replaced.
        let shown = self.imported.as_deref().unwrap_or(game_files);
        text.push_str(&format!("Game files: {shown}"));
        if !config.is_empty() {
            text.push_str(&format!("\nConfig: {config}"));
        }
        text
    }

    /// Button labels for the card, left slot first. Cancellation keeps the
    /// dialog busy until the platform confirms that copying has stopped.
    pub(crate) fn buttons(&self) -> [Option<&'static str>; 2] {
        match &self.step {
            ImportStep::Closed => [Some("Import Game Files"), None],
            ImportStep::ChooseFolder | ImportStep::Failed(_) => {
                [Some("Choose Folder"), Some("Cancel")]
            }
            ImportStep::Copying { .. } => [None, Some("Cancel")],
            ImportStep::Cancelling => [None, None],
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
            ImportStep::Copying { .. } | ImportStep::Cancelling => ButtonPress::Ignored,
            ImportStep::Done => {
                self.close();
                ButtonPress::Ignored
            }
        }
    }

    pub(crate) fn press_right(&mut self) -> ButtonPress {
        match self.step {
            ImportStep::ChooseFolder | ImportStep::Failed(_) => self.close(),
            ImportStep::Copying { .. } => {
                self.cancel_copy();
                return ButtonPress::CancelCopy;
            }
            _ => {}
        }
        ButtonPress::Ignored
    }
}

/// What the pressed button asks the caller to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ButtonPress {
    /// Open the platform's folder picker.
    PickFolder,
    CancelCopy,
    /// Handled inside the dialog.
    Ignored,
}

/// Ask the platform for the folder holding the retail installation.
///
/// `false` means this build has no picker; `true` means the picker opened and
/// the platform will deliver the choice through `folder_picker` later.
pub(crate) fn choose_folder() -> bool {
    if !crate::util::folder_picker::request() {
        log::info!("asset import: this build has no folder picker wired yet");
        return false;
    }
    true
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
        assert!(dialog.body("", "").contains("Failed: not enough space"));
        assert_eq!(dialog.buttons(), [Some("Choose Folder"), Some("Cancel")]);
        assert_eq!(dialog.press_left(), ButtonPress::PickFolder);
    }

    #[test]
    fn cancelling_cannot_start_another_copy_before_the_worker_finishes() {
        let mut dialog = AssetImport::default();
        dialog.start_copy(None);
        assert_eq!(dialog.press_right(), ButtonPress::CancelCopy);
        dialog.open();
        dialog.advance_copy(16);
        assert_eq!(dialog.step(), &ImportStep::Cancelling);
        assert_eq!(dialog.buttons(), [None, None]);
        assert_eq!(dialog.press_left(), ButtonPress::Ignored);
        dialog.close(); // The platform's Cancelled completion event.
        dialog.open();
        assert_eq!(dialog.press_left(), ButtonPress::PickFolder);
    }
}
