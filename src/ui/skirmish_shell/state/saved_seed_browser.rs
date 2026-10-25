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
}

impl<I> SavedSeedBrowserRow<I> {
    fn visible(&self) -> bool {
        self.file_name.is_none() || self.visible
    }
}

/// NewEdit614B30 replaces the resource Edit control with its own UTF-16 editor.
/// It has a caret, but no selection range or mouse caret placement.
#[derive(Debug, Clone, Default)]
pub struct SavedSeedDescriptionEdit {
    pub units: Vec<u16>,
    pub caret: usize,
    pub first_visible_unit: usize,
    pub focused: bool,
    /// `ES_PASSWORD`: the paint passes `(style >> 5) & 1` to
    /// `NewEdit__DrawTextAndCaret` `0x00623880`, which draws every unit as
    /// `*` (`0x00614FDC..0x00615009`).
    pub password: bool,
}

impl SavedSeedDescriptionEdit {
    pub fn set_text(&mut self, description: &SeedDescription) {
        // Programmatic assignment also passes through7B78D0 after EM_LIMITTEXT.
        self.set_units_limited(
            description.units().iter().copied(),
            SAVED_SEED_DESCRIPTION_MAX_UNITS,
        );
    }

    /// NewEdit614B30's shared assignment: limits count UTF-16 units, and zero
    /// means unlimited. D7 uses five/eleven units; its port has no limit.
    pub fn set_units_limited(&mut self, units: impl IntoIterator<Item = u16>, limit: usize) {
        self.units = units
            .into_iter()
            .take(if limit == 0 { usize::MAX } else { limit })
            .collect();
        self.caret = self.units.len();
        self.first_visible_unit = 0;
    }

    pub fn insert_text(&mut self, text: &str) {
        self.insert_text_limited(text, SAVED_SEED_DESCRIPTION_MAX_UNITS);
    }

    /// Native 614B30 insertion, retaining the caller's configured EM_LIMITTEXT.
    pub fn insert_text_limited(&mut self, text: &str, limit: usize) {
        for unit in text.encode_utf16() {
            self.insert_unit_filtered(unit, limit, false, &[]);
        }
    }

    /// NewEdit614B30 at615226..61527A: reject-character table, UTF-16
    /// limit, control-character gate and optional byte-only input (<0x100).
    /// One unit lets callers run their native EN_CHANGE handler after each
    /// admitted WM_CHAR instead of applying that handler after a pasted batch.
    pub fn insert_unit_filtered(
        &mut self,
        unit: u16,
        limit: usize,
        byte_only: bool,
        rejected: &[u16],
    ) -> bool {
        if unit <= 0x1f
            || (byte_only && unit >= 0x100)
            || rejected.contains(&unit)
            || (limit != 0 && self.units.len() >= limit)
        {
            return false;
        }
        self.units.insert(self.caret, unit);
        self.caret += 1;
        true
    }

    /// NewEdit multiline Enter614DEB appends CRLF to the end, independently
    /// of the caret and EM_LIMITTEXT, before notifying its parent with501.
    pub fn append_enter_line_end(&mut self) {
        self.units.extend([13, 10]);
    }

    /// NewEdit4B3 getter614D49: caller capacity includes its forced final NUL.
    pub fn wide_text(&self, capacity: usize) -> Vec<u16> {
        self.units
            .iter()
            .copied()
            .take(capacity.saturating_sub(1))
            .take_while(|unit| *unit != 0)
            .collect()
    }

    /// NewEdit message 0x4B5 (614D28): narrow each UTF-16 unit to its low byte,
    /// then terminate the caller's buffer at capacity-1. This is not ACP.
    pub fn narrow_text(&self, capacity: usize) -> String {
        crate::util::native_string::widen_bytes(
            &self
                .units
                .iter()
                .take(capacity.saturating_sub(1))
                .map(|unit| *unit as u8)
                .take_while(|byte| *byte != 0)
                .collect::<Vec<_>>(),
        )
    }

    pub fn backspace(&mut self) {
        if self.caret > 0 {
            self.caret -= 1;
            self.units.remove(self.caret);
        }
    }

    pub fn delete(&mut self) {
        if self.caret < self.units.len() {
            self.units.remove(self.caret);
        }
    }

    pub fn left(&mut self) {
        self.caret = self.caret.saturating_sub(1);
    }
    pub fn right(&mut self) {
        self.caret = (self.caret + 1).min(self.units.len());
    }
    pub fn home(&mut self) {
        self.caret = 0;
    }
    pub fn end(&mut self) {
        self.caret = self.units.len();
    }

    pub fn description(&self) -> SeedDescription {
        // Save driver727D60 trims wide units <=0x20 after reading at most79.
        let start = self
            .units
            .iter()
            .position(|u| *u > 0x20)
            .unwrap_or(self.units.len());
        let end = self
            .units
            .iter()
            .rposition(|u| *u > 0x20)
            .map_or(start, |i| i + 1);
        SeedDescription::from_units(self.units[start..end].iter().copied())
    }

    pub fn display_text(&self) -> String {
        String::from_utf16_lossy(&self.units)
    }

    /// The units `start..end` as the NewEdit paints them (masked for
    /// `ES_PASSWORD`).
    pub fn painted_range(&self, start: usize, end: usize) -> String {
        let end = end.min(self.units.len());
        let start = start.min(end);
        if self.password {
            "*".repeat(end - start)
        } else {
            String::from_utf16_lossy(&self.units[start..end])
        }
    }

    /// The painted text from the first visible unit on.
    pub fn painted_text(&self) -> String {
        self.painted_range(self.first_visible_unit, self.units.len())
    }

    /// NewEdit__DrawTextAndCaret `0x00623880` with an open IME composition
    /// (`0x006238D0..0x006239C2`): the composition units are spliced in at the
    /// caret, the painted caret sits at the composition cursor and the scroll
    /// keeps that position visible. Returns the painted editor and the
    /// composition's unit range in it.
    pub fn with_composition(
        &self,
        composition: &[u16],
        cursor: usize,
        available: u32,
        measure: impl Fn(&str) -> u32,
    ) -> (Self, std::ops::Range<usize>) {
        let caret = self.caret.min(self.units.len());
        let mut view = self.clone();
        view.units.splice(caret..caret, composition.iter().copied());
        view.caret = caret + cursor.min(composition.len());
        view.scroll_to_caret(available, measure);
        (view, caret..caret + composition.len())
    }

    /// Keep the insertion point visible using the active font's measured width.
    pub fn scroll_to_caret(&mut self, available: u32, measure: impl Fn(&str) -> u32) {
        self.first_visible_unit = self.first_visible_unit.min(self.caret);
        while self.first_visible_unit < self.caret {
            let prefix = self.painted_range(self.first_visible_unit, self.caret);
            if measure(&prefix) <= available {
                break;
            }
            self.first_visible_unit += 1;
        }
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
    pub scroll_repeat_at: Option<std::time::Instant>,
    pub opened_at: std::time::Instant,
    pub last_list_press: Option<(std::time::Instant, i32, i32)>,
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
            opened_at: std::time::Instant::now(),
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

    pub fn remove_entry(&mut self, file_name: &I, visible_rows: usize) {
        self.entries
            .retain(|row| row.file_name.as_ref() != Some(file_name));
        self.selected = (!self.entries.is_empty()).then_some(0);
        self.top_index = self
            .top_index
            .min(self.entries.len().saturating_sub(visible_rows));
    }
}

#[cfg(test)]
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
    fn open_composition_splices_at_the_caret_and_scrolls_to_its_cursor() {
        let mut edit = SavedSeedDescriptionEdit::default();
        edit.insert_text("abcd");
        edit.left();
        edit.left();
        let composition: Vec<u16> = "xyz".encode_utf16().collect();
        // Ten pixels a unit: thirty pixels show three units.
        let (view, units) = edit.with_composition(&composition, 2, 30, |text| {
            text.encode_utf16().count() as u32 * 10
        });
        assert_eq!(view.display_text(), "abxyzcd");
        assert_eq!(units, 2..5);
        assert_eq!(view.caret, 4, "the caret sits at the composition cursor");
        assert_eq!(view.first_visible_unit, 1, "scrolled to keep it in view");
        assert_eq!(edit.display_text(), "abcd", "the edit keeps its own text");
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
