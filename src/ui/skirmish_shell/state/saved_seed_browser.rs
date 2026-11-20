//! Shared saved-file browser records, caret-only description edit, and actions.
//! File identities stay opaque; SED callers retain the NativeFileName default.
//! Native driver558DD0 and list rebuild5596A0 keep filenames separate from text.

use super::super::layout::{SavedSeedControl, SavedSeedMode};
use crate::map::rmg::{SeedDescription, saved_seeds::SavedSeed};
use crate::util::native_file_name::NativeFileName;

pub const SAVED_SEED_DESCRIPTION_MAX_UNITS: usize = 79;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavedSeedOutcome<I = NativeFileName> {
    Load(I),
    Save {
        file_name: Option<I>,
        description: SeedDescription,
    },
    Delete(I),
    Close,
}

#[derive(Debug, Clone)]
pub enum SavedSeedPromptPurpose<I = NativeFileName> {
    EmptyDescription,
    /// Acknowledge an operation failure without changing the selected row.
    Error,
    Overwrite {
        file_name: I,
        description: SeedDescription,
    },
    Delete {
        file_name: I,
    },
    Saved,
}

#[derive(Debug, Clone)]
pub struct SavedSeedPrompt<I = NativeFileName> {
    pub purpose: SavedSeedPromptPurpose<I>,
    pub body: String,
    pub affirmative: String,
    pub negative: Option<String>,
}

impl<I> SavedSeedPrompt<I> {
    /// Both shapes use the shared native message-box templates.
    pub fn layout(
        &self,
        width: u32,
        height: u32,
    ) -> (
        super::super::layout::RectPx,
        super::super::layout::RectPx,
        super::super::layout::RectPx,
        Option<super::super::layout::RectPx>,
    ) {
        if self.negative.is_some() {
            let layout = crate::ui::shell::modal::quit_confirm_layout(width as i32, height as i32);
            (layout.dialog, layout.body, layout.ok, Some(layout.cancel))
        } else {
            let layout = crate::ui::shell::modal::body_ok_layout(width as i32, height as i32);
            (layout.dialog, layout.body, layout.ok, None)
        }
    }

    pub fn control_at(&self, width: u32, height: u32, x: i32, y: i32) -> Option<SavedSeedControl> {
        let (_, _, yes, no) = self.layout(width, height);
        if yes.contains(x, y) {
            Some(SavedSeedControl::Action)
        } else if no.is_some_and(|rect| rect.contains(x, y)) {
            Some(SavedSeedControl::Back0x686)
        } else {
            None
        }
    }
}

/// A visible or not-yet-filtered native metadata row. None identifies New.
#[derive(Debug, Clone)]
pub struct SavedSeedBrowserRow<I = NativeFileName> {
    pub file_name: Option<I>,
    pub description: SeedDescription,
    pub last_write_time: u64,
    /// Metadata admission belongs to the file format. SED uses nonempty
    /// descriptions; a valid game header can admit an empty description.
    pub visible: bool,
    /// Whether Load may open this row. VERA-only: a saved game another build
    /// wrote stays listed in Load and Delete (gamemd hides it at
    /// `0x005599DF`), so the player can see why it is gone and delete it.
    pub loadable: bool,
}

impl<I> SavedSeedBrowserRow<I> {
    fn visible(&self) -> bool {
        self.file_name.is_none() || self.visible
    }
}

/// NewEdit614B30, the caret-only UTF-16 editor every shell edit runs on;
/// it lives in `vera20k-gui` ([`vera20k_gui::shell::new_edit`]).
pub use vera20k_gui::shell::new_edit::NewEdit as SavedSeedDescriptionEdit;

/// The saved-file browser's description field on the shared NewEdit.
pub trait SeedDescriptionEdit {
    fn set_text(&mut self, description: &SeedDescription);
    fn insert_text(&mut self, text: &str);
    fn description(&self) -> SeedDescription;
}

impl SeedDescriptionEdit for SavedSeedDescriptionEdit {
    fn set_text(&mut self, description: &SeedDescription) {
        // Programmatic assignment also passes through 0x007B78D0 after EM_LIMITTEXT.
        self.set_units_limited(
            description.units().iter().copied(),
            SAVED_SEED_DESCRIPTION_MAX_UNITS,
        );
    }

    fn insert_text(&mut self, text: &str) {
        self.insert_text_limited(text, SAVED_SEED_DESCRIPTION_MAX_UNITS);
    }

    fn description(&self) -> SeedDescription {
        // The save driver trims with wcstrim 0x00727D60 after reading at most 79.
        SeedDescription::from_units(
            crate::util::native_string::wcstrim(&self.units)
                .iter()
                .copied(),
        )
    }
}

#[derive(Debug, Clone)]
pub struct SavedSeedBrowserState<I = NativeFileName> {
    pub mode: SavedSeedMode,
    pub entries: Vec<SavedSeedBrowserRow<I>>,
    pub selected: Option<usize>,
    pub top_index: usize,
    pub description_edit: SavedSeedDescriptionEdit,
    pub pressed_control: Option<SavedSeedControl>,
    pub prompt: Option<SavedSeedPrompt<I>>,
    pub scroll_repeat_at: Option<crate::util::time::Instant>,
    pub opened_at: crate::util::time::Instant,
    pub last_list_press: Option<(crate::util::time::Instant, i32, i32)>,
    current_description: SeedDescription,
}

impl SavedSeedBrowserState<NativeFileName> {
    pub fn open(
        mode: SavedSeedMode,
        entries: Vec<SavedSeed>,
        current_description: SeedDescription,
        empty_slot_label: SeedDescription,
        new_slot_time: u64,
        visible_rows: usize,
    ) -> Self {
        Self::open_rows(
            mode,
            entries
                .into_iter()
                .map(|entry| SavedSeedBrowserRow {
                    file_name: Some(entry.file_name),
                    visible: !entry.description.is_empty(),
                    loadable: true,
                    description: entry.description,
                    last_write_time: entry.last_write_time,
                })
                .collect(),
            current_description,
            empty_slot_label,
            new_slot_time,
            visible_rows,
        )
    }
}

impl<I> SavedSeedBrowserState<I> {
    /// The name field's caret phase: one second on, one second off from the
    /// browser's opening. The caret is drawn only while the field has focus.
    pub fn caret_blink_on(&self) -> bool {
        self.opened_at.elapsed().as_millis() % 2000 < 1000
    }

    /// Raise a message box over the browser. It takes the edit focus and ends
    /// any held press or scroll repeat; its buttons own input until answered.
    pub fn show_prompt(&mut self, prompt: SavedSeedPrompt<I>) {
        self.description_edit.focused = false;
        self.pressed_control = None;
        self.scroll_repeat_at = None;
        self.prompt = Some(prompt);
    }

    /// Window focus loss: the release that would end the held press, the
    /// scroll repeat and the double-click pair never arrives.
    pub fn reset_interaction(&mut self) {
        self.pressed_control = None;
        self.scroll_repeat_at = None;
        self.last_list_press = None;
    }
}

impl<I: Clone + PartialEq> SavedSeedBrowserState<I> {
    /// Shared native list/editor mechanism. The caller supplies opaque file
    /// identities and metadata; no filename encoding or filesystem I/O occurs.
    pub fn open_rows(
        mode: SavedSeedMode,
        entries: Vec<SavedSeedBrowserRow<I>>,
        current_description: SeedDescription,
        empty_slot_label: SeedDescription,
        new_slot_time: u64,
        visible_rows: usize,
    ) -> Self {
        let mut rows = Vec::with_capacity(entries.len() + usize::from(mode == SavedSeedMode::Save));
        if mode == SavedSeedMode::Save {
            rows.push(SavedSeedBrowserRow {
                file_name: None,
                description: empty_slot_label,
                last_write_time: new_slot_time,
                visible: true,
                loadable: true,
            });
        }
        rows.extend(entries);
        let mut order: Vec<usize> = (0..rows.len()).collect();
        crate::util::retail_pointer_sort::sort_indices_by(&mut order, |left, right| {
            rows[right].last_write_time.cmp(&rows[left].last_write_time)
        });
        let mut storage: Vec<_> = rows.into_iter().map(Some).collect();
        let mut rows: Vec<_> = order
            .into_iter()
            .map(|i| storage[i].take().unwrap())
            .collect();
        // Native Load uses the unfiltered metadata index as a visible-list
        // index. Preserve the wrong/no-selection cases caused by invalid rows.
        let initial = if mode == SavedSeedMode::Load {
            rows.iter()
                .position(|row| row.file_name.is_some() && row.visible())
        } else {
            Some(0)
        };
        rows.retain(SavedSeedBrowserRow::visible);
        let selected = initial.filter(|index| *index < rows.len());
        let top_index = initial
            .unwrap_or(0)
            .min(rows.len().saturating_sub(visible_rows));
        let mut result = Self {
            mode,
            entries: rows,
            selected,
            top_index,
            description_edit: SavedSeedDescriptionEdit::default(),
            pressed_control: None,
            prompt: None,
            scroll_repeat_at: None,
            opened_at: crate::util::time::Instant::now(),
            last_list_press: None,
            current_description,
        };
        if mode == SavedSeedMode::Save {
            result.assign_selected_description();
        }
        result
    }

    pub fn selected_entry(&self) -> Option<&SavedSeedBrowserRow<I>> {
        self.selected.and_then(|index| self.entries.get(index))
    }

    fn assign_selected_description(&mut self) {
        let text = self
            .selected_entry()
            .filter(|row| row.file_name.is_some())
            .map(|row| &row.description)
            .unwrap_or(&self.current_description);
        let text = text.clone();
        self.description_edit.set_text(&text);
        self.description_edit.focused = true;
    }

    pub fn select(&mut self, index: usize) {
        if index >= self.entries.len() {
            return;
        }
        self.selected = Some(index);
        if self.mode == SavedSeedMode::Save {
            self.assign_selected_description();
        }
    }

    pub fn action_enabled(&self) -> bool {
        //558DD0 enables from visible count, even when Load has no selection
        // or Save has a blank description. The action loop handles those cases.
        !self.entries.is_empty()
    }

    pub fn action_outcome(&self) -> Option<SavedSeedOutcome<I>> {
        let entry = self.selected_entry()?;
        match self.mode {
            SavedSeedMode::Save => Some(SavedSeedOutcome::Save {
                file_name: entry.file_name.clone(),
                description: self.description_edit.description(),
            }),
            SavedSeedMode::Load => Some(SavedSeedOutcome::Load(entry.file_name.clone()?)),
            SavedSeedMode::Delete => Some(SavedSeedOutcome::Delete(entry.file_name.clone()?)),
        }
    }

    /// VERA's Setups page: Load and Delete act on the selected saved row,
    /// never the new one.
    pub fn selected_saved(&self) -> Option<&I> {
        self.selected_entry()?.file_name.as_ref()
    }

    /// VERA's Setups page: what its Load or Delete does.
    pub fn setup_outcome(&self, control: SavedSeedControl) -> Option<SavedSeedOutcome<I>> {
        let saved = self.selected_saved()?.clone();
        match control {
            SavedSeedControl::Load => Some(SavedSeedOutcome::Load(saved)),
            SavedSeedControl::Delete => Some(SavedSeedOutcome::Delete(saved)),
            _ => None,
        }
    }

    pub fn remove_entry(&mut self, file_name: &I, visible_rows: usize) {
        self.entries
            .retain(|row| row.file_name.as_ref() != Some(file_name));
        self.selected = (!self.entries.is_empty()).then_some(0);
        self.top_index = self
            .top_index
            .min(self.entries.len().saturating_sub(visible_rows));
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;

    fn seed(name: &str, time: u64, valid: bool) -> SavedSeed {
        SavedSeed {
            file_name: format!("{name}.SED").into(),
            description: if valid {
                name.into()
            } else {
                SeedDescription::default()
            },
            last_write_time: time,
        }
    }

    fn open(mode: SavedSeedMode, entries: Vec<SavedSeed>) -> SavedSeedBrowserState {
        SavedSeedBrowserState::open(
            mode,
            entries,
            "Working".into(),
            "[EMPTY SLOT]".into(),
            100,
            2,
        )
    }

    #[test]
    fn new_row_participates_in_native_sort_and_uses_working_description() {
        let mut state = open(SavedSeedMode::Save, vec![seed("Future", 110, true)]);
        assert_eq!(
            state.entries[0].file_name.as_ref(),
            Some(&"Future.SED".into())
        );
        assert_eq!(state.description_edit.description(), "Future");
        state.select(1);
        assert_eq!(state.description_edit.description(), "Working");
        assert!(matches!(
            state.action_outcome(),
            Some(SavedSeedOutcome::Save {
                file_name: None,
                ..
            })
        ));
    }

    #[test]
    fn invalid_records_sort_before_filtering_and_load_uses_raw_index() {
        let wrong = open(
            SavedSeedMode::Load,
            vec![
                seed("Invalid", 30, false),
                seed("A", 20, true),
                seed("B", 10, true),
            ],
        );
        assert_eq!(
            wrong.selected_entry().unwrap().file_name.as_ref(),
            Some(&"B.SED".into())
        );
        let no_selection = open(
            SavedSeedMode::Load,
            vec![seed("Invalid", 30, false), seed("A", 20, true)],
        );
        assert_eq!(no_selection.selected, None);
        assert!(no_selection.action_enabled());
        assert_eq!(no_selection.action_outcome(), None);
        let ties = open(
            SavedSeedMode::Load,
            vec![
                seed("Invalid", 10, false),
                seed("A", 10, true),
                seed("B", 10, true),
            ],
        );
        assert_eq!(ties.entries[0].file_name.as_ref(), Some(&"A.SED".into()));
    }

    #[test]
    fn blank_save_is_enabled_and_requests_validation_without_a_filename() {
        let mut state = open(SavedSeedMode::Save, vec![]);
        state.description_edit.set_text(&"   ".into());
        assert!(state.action_enabled());
        assert_eq!(
            state.action_outcome(),
            Some(SavedSeedOutcome::Save {
                file_name: None,
                description: SeedDescription::default(),
            })
        );
    }

    #[test]
    fn edit_operates_on_utf16_units_and_programmatic_assignment_is_bounded() {
        let mut edit = SavedSeedDescriptionEdit::default();
        edit.set_text(&SeedDescription::from_units([0xd83d, 0xde00, 0x41]));
        edit.home();
        edit.right();
        edit.delete();
        assert_eq!(edit.description().units(), [0xd83d, 0x41]);
        edit.set_text(&"x".repeat(100).into());
        assert_eq!(edit.units.len(), 79);
        edit.insert_text("more");
        assert_eq!(edit.units.len(), 79);
        edit.backspace();
        edit.insert_text(":");
        assert_eq!(edit.units[78], u16::from(b':'));
    }

    #[test]
    fn path_identity_survives_selection_save_and_delete_without_text_conversion() {
        let path = std::path::PathBuf::from("saves/保存 — ridge.bin");
        let mut state = SavedSeedBrowserState::open_rows(
            SavedSeedMode::Save,
            vec![SavedSeedBrowserRow {
                file_name: Some(path.clone()),
                description: "Saved description".into(),
                last_write_time: 110,
                visible: true,
                loadable: true,
            }],
            "Working".into(),
            "[EMPTY SLOT]".into(),
            100,
            2,
        );
        assert_eq!(
            state.selected_entry().unwrap().file_name.as_ref(),
            Some(&path)
        );
        state
            .description_edit
            .set_text(&"Changed description".into());
        assert_eq!(
            state.action_outcome(),
            Some(SavedSeedOutcome::Save {
                file_name: Some(path.clone()),
                description: "Changed description".into(),
            })
        );
        state.remove_entry(&path, 2);
        assert_eq!(state.entries.len(), 1);
        assert_eq!(state.entries[0].file_name, None);
        state.select(0);
        assert_eq!(state.description_edit.description(), "Working");
    }

    #[cfg(windows)]
    #[test]
    fn opaque_windows_path_units_do_not_round_trip_through_description_encoding() {
        use std::os::windows::ffi::OsStringExt;
        let path = std::path::PathBuf::from(std::ffi::OsString::from_wide(&[
            0x73, 0x61, 0x76, 0x65, 0xd800, 0x2e, 0x62, 0x69, 0x6e,
        ]));
        let state = SavedSeedBrowserState::open_rows(
            SavedSeedMode::Load,
            vec![SavedSeedBrowserRow {
                file_name: Some(path.clone()),
                description: SeedDescription::default(),
                last_write_time: 1,
                visible: true,
                loadable: true,
            }],
            SeedDescription::default(),
            "[EMPTY SLOT]".into(),
            2,
            2,
        );
        assert_eq!(state.action_outcome(), Some(SavedSeedOutcome::Load(path)));
    }

    #[test]
    fn delete_resets_selection_to_first_visible_row() {
        let mut state = open(
            SavedSeedMode::Delete,
            vec![
                seed("A", 30, true),
                seed("B", 20, true),
                seed("C", 10, true),
            ],
        );
        state.select(1);
        state.remove_entry(&"B.SED".into(), 2);
        assert_eq!(state.selected, Some(0));
        state.remove_entry(&"A.SED".into(), 2);
        state.remove_entry(&"C.SED".into(), 2);
        assert!(state.entries.is_empty());
        assert!(!state.action_enabled());
    }
}
