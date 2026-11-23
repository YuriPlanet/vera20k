//! The shared saved-file browser (driver 0x00558DD0): its rows and message
//! box, with the list 0x525, the description edit 0x526 and the buttons as
//! `vera20k-gui` widgets, which own the selection, top, text, caret, press,
//! scroll and double-click. SED identities carry the save folder and native
//! filename. List rebuild 5596A0 keeps filenames separate from text.

use super::super::layout::{RectPx, SavedSeedMode};
use crate::map::rmg::{
    SeedDescription,
    saved_seeds::{SavedSeed, SavedSeedFile},
};
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::page;
use vera20k_gui::{NodeId, NodeKind, NodeSpec, Prop, Value};

pub const SAVED_SEED_DESCRIPTION_MAX_UNITS: usize = 79;

/// The browser and its message box in the browser's own controller.
pub const BROWSER_DIALOG: DialogResourceId = DialogResourceId(0x00B7);
pub const PROMPT_DIALOG: DialogResourceId = DialogResourceId(0x5D34);
/// The list, the description edit (Save only), the action (Load `0x40F`,
/// Save `0x6C7` or Delete `0x6C8`), Back, VERA's Setups Load and Delete, and
/// the box's affirmative and negative buttons.
pub const LIST: u16 = 0x525;
pub const EDIT: u16 = 0x526;
pub const ACTION: u16 = 0x40F;
pub const BACK: u16 = 0x686;
pub const LOAD: u16 = 0x7501;
pub const DELETE: u16 = 0x7502;
pub use crate::ui::shell::saved_games::CONVERT_BUTTON as CONVERT;
pub const YES: u16 = 1;
pub const NO: u16 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavedSeedOutcome<I = SavedSeedFile> {
    Load(I),
    BeginConvert(I),
    Save {
        file_name: Option<I>,
        description: SeedDescription,
    },
    Delete(I),
    Close,
}

#[derive(Debug, Clone)]
pub enum SavedSeedPromptPurpose<I = SavedSeedFile> {
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
pub struct SavedSeedPrompt<I = SavedSeedFile> {
    pub purpose: SavedSeedPromptPurpose<I>,
    pub body: String,
    pub affirmative: String,
    pub negative: Option<String>,
}

impl<I> SavedSeedPrompt<I> {
    /// The box as it paints: the body over the affirmative button and, for
    /// a question, the negative one.
    pub fn shell_box(&self) -> crate::ui::shell::modal::ShellBox {
        use crate::ui::shell::modal::ShellBox;
        let dialog = [0xCE, 0x120][usize::from(self.negative.is_some())];
        let dialog = crate::ui::shell::descriptor::DialogResourceId(dialog);
        let shell_box = ShellBox::new(dialog, self.body.clone()).button(YES, &*self.affirmative);
        match &self.negative {
            Some(no) => shell_box.button(NO, no.as_str()),
            None => shell_box,
        }
    }

    /// Both shapes use the shared native message-box templates: dialog,
    /// body, affirmative and negative.
    pub fn layout(&self, width: u32, height: u32) -> (RectPx, RectPx, RectPx, Option<RectPx>) {
        let retail = crate::ui::shell::geom::RETAIL_SCALE;
        let parts = self.shell_box().parts(width as i32, height as i32, retail);
        (parts.dialog, parts.body, parts.button(0), parts.rect_of(NO))
    }
}

/// A visible or not-yet-filtered native metadata row. None identifies New.
#[derive(Debug, Clone)]
pub struct SavedSeedBrowserRow<I = SavedSeedFile> {
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
    /// The save format has a converter for this row and the current build.
    pub migratable: bool,
}

impl<I> SavedSeedBrowserRow<I> {
    fn visible(&self) -> bool {
        self.file_name.is_none() || self.visible
    }
}

/// NewEdit614B30, the caret-only UTF-16 editor every shell edit runs on;
/// it lives in `vera20k-gui` ([`vera20k_gui::shell::new_edit`]).
use vera20k_gui::shell::new_edit::NewEdit;

#[derive(Debug)]
pub struct SavedSeedBrowserState<I = SavedSeedFile> {
    pub mode: SavedSeedMode,
    pub entries: Vec<SavedSeedBrowserRow<I>>,
    pub prompt: Option<SavedSeedPrompt<I>>,
    /// The browser's widgets and, while `prompt` is up, the box's buttons.
    pub ui: DialogController,
    current_description: SeedDescription,
    conversion_source: Option<I>,
}

impl SavedSeedBrowserState<SavedSeedFile> {
    pub fn open(
        mode: SavedSeedMode,
        entries: Vec<SavedSeed>,
        current_description: SeedDescription,
        empty_slot_label: SeedDescription,
        new_slot_time: u64,
        visible_rows: usize,
    ) -> Self {
        let rows = entries.into_iter().map(|entry| SavedSeedBrowserRow {
            file_name: Some(entry.file_name),
            visible: !entry.description.is_empty(),
            loadable: true,
            migratable: false,
            description: entry.description,
            last_write_time: entry.last_write_time,
        });
        let rows = rows.collect();
        Self::open_rows(
            mode,
            rows,
            current_description,
            empty_slot_label,
            new_slot_time,
            visible_rows,
        )
    }
}

/// The browser's widgets in hit order (the edit and list before the column),
/// the buttons with their captions, the edit framed in its rect. Load's
/// carry their status help (`0x006040B0`).
fn spec(mode: SavedSeedMode) -> NodeSpec {
    use crate::ui::shell::saved_games::{LOAD_LIST_HELP_KEY, LOAD_SAVED_GAME_PAGE as PAGE};
    let help = |node: NodeSpec, key| match mode {
        SavedSeedMode::Load => node.tooltip(key),
        _ => node,
    };
    let button = |(id, key): (u16, &str)| {
        let caption = Value::Text(key.to_owned());
        NodeSpec::new(NodeKind::Button)
            .id(id.into())
            .with(Prop::Label, caption)
    };
    let list = NodeSpec::new(NodeKind::List).id(LIST.into());
    let inset = Value::Rect(super::super::layout::FRAMED_EDIT_TEXT_INSET);
    let edit = NodeSpec::new(NodeKind::Edit)
        .id(EDIT.into())
        .with(
            Prop::MaxLen,
            Value::Int(SAVED_SEED_DESCRIPTION_MAX_UNITS as i32),
        )
        .with(Prop::TextInset, inset);
    let action = button((ACTION, mode.action_label().0));
    NodeSpec::new(NodeKind::Dialog)
        .child(edit)
        .child(help(list, LOAD_LIST_HELP_KEY))
        .child(help(action, PAGE.stacked[0].tooltip_key))
        .child(help(
            button((CONVERT, "VERA:ConvertSave")),
            "VERA:ConvertSaveHelp",
        ))
        .children([(LOAD, "GUI:Load"), (DELETE, "GUI:Delete")].map(button))
        .child(help(button((BACK, "GUI:Back")), PAGE.back.tooltip_key))
}

/// The dialog control `id` lives in.
fn dialog(id: u16) -> DialogResourceId {
    if matches!(id, YES | NO) {
        PROMPT_DIALOG
    } else {
        BROWSER_DIALOG
    }
}

fn int(value: Option<&Value>) -> Option<i32> {
    match value? {
        Value::Int(value) => Some(*value),
        _ => None,
    }
}

impl<I> SavedSeedBrowserState<I> {
    pub fn node(&self, id: u16) -> Option<NodeId> {
        self.ui.control(dialog(id), id)
    }

    pub fn selected(&self) -> Option<usize> {
        usize::try_from(int(self.ui.prop(BROWSER_DIALOG, LIST, Prop::Selected))?).ok()
    }

    pub fn selected_entry(&self) -> Option<&SavedSeedBrowserRow<I>> {
        self.selected().and_then(|index| self.entries.get(index))
    }

    pub fn conversion_source(&self) -> Option<&I> {
        self.conversion_source.as_ref()
    }

    pub fn is_naming_conversion(&self) -> bool {
        self.conversion_source.is_some()
    }

    pub fn game_title_label(&self) -> (&'static str, &'static str) {
        if self.is_naming_conversion() {
            ("VERA:SaveConvertedGame", "Save Converted Game")
        } else {
            crate::ui::shell::saved_games::saved_game_title_label(self.mode)
        }
    }

    pub fn convert_enabled(&self) -> bool {
        self.mode == SavedSeedMode::Load
            && self
                .selected_entry()
                .is_some_and(|row| row.file_name.is_some() && row.migratable && !row.loadable)
    }

    pub fn top(&self) -> usize {
        int(self.ui.prop(BROWSER_DIALOG, LIST, Prop::Top)).map_or(0, |top| top.max(0) as usize)
    }

    fn set_list(&mut self, selected: Option<usize>, top: usize) {
        let rows = Value::Int(self.entries.len() as i32);
        self.ui.set_prop(BROWSER_DIALOG, LIST, Prop::RowCount, rows);
        let selected = Value::Int(selected.map_or(-1, |row| row as i32));
        self.ui
            .set_prop(BROWSER_DIALOG, LIST, Prop::Selected, selected);
        self.ui
            .set_prop(BROWSER_DIALOG, LIST, Prop::Top, Value::Int(top as i32));
    }

    /// Whether button `id` holds the press (a box's buttons, or the
    /// browser's while no box is up), pushed on or off the pointer.
    pub fn pressed(&self, id: u16) -> bool {
        self.ui
            .pressed_node_in(dialog(id))
            .is_some_and(|node| Some(node) == self.node(id))
    }

    /// The description edit as NewEdit paints it: the first shown unit is
    /// its `Top`, the focus the browser's own.
    pub fn edit(&self) -> NewEdit {
        let edit = self
            .node(EDIT)
            .and_then(|node| self.ui.gui().edit_state(node));
        NewEdit {
            focused: self.prompt.is_none() && self.ui.focused_control(BROWSER_DIALOG) == Some(EDIT),
            password: false,
            ..edit.unwrap_or_default()
        }
    }

    /// NewEdit's caret blink, while the edit has the focus and no box is up.
    pub fn caret_visible(&self) -> bool {
        self.node(EDIT)
            .is_some_and(|node| self.ui.caret_visible(node))
    }

    /// The description the save driver reads: at most 79 units, trimmed by
    /// wcstrim 0x00727D60.
    pub fn description(&self) -> SeedDescription {
        let units = self.edit().units;
        SeedDescription::from_units(crate::util::native_string::wcstrim(&units).iter().copied())
    }

    /// Programmatic assignment also passes through 0x007B78D0 after
    /// EM_LIMITTEXT; the edit takes the focus.
    fn set_description(&mut self, description: &SeedDescription) {
        let units = description.units();
        let units = &units[..units.len().min(SAVED_SEED_DESCRIPTION_MAX_UNITS)];
        self.ui
            .set_text(BROWSER_DIALOG, EDIT, &String::from_utf16_lossy(units));
        self.ui.focus_control(BROWSER_DIALOG, Some(EDIT));
    }

    /// The status help of the control under the pointer, none under a box.
    pub fn hovered_help(&self) -> Option<&str> {
        let root = self
            .ui
            .root_in(BROWSER_DIALOG)
            .filter(|_| self.prompt.is_none())?;
        page::hovered_key_in(self.ui.gui(), root)
    }

    /// Raise a message box over the browser. It takes the edit focus and ends
    /// any held press or scroll repeat; its buttons own input until answered.
    pub fn show_prompt(&mut self, prompt: SavedSeedPrompt<I>) {
        self.ui.drop_pointer_in(BROWSER_DIALOG);
        self.ui.focus_control(BROWSER_DIALOG, None);
        self.ui.close(PROMPT_DIALOG);
        self.ui.push_modal(PROMPT_DIALOG);
        if let Some(root) = self.ui.root_in(PROMPT_DIALOG) {
            // 5D4DDB IsDialogMessageA / 5D370D: Enter's IDOK and Escape's
            // IDCANCEL both answer 1, the negative No (hidden or not).
            let button = |id: u16| {
                NodeSpec::new(NodeKind::Button)
                    .id(id.into())
                    .with(Prop::Default, Value::Bool(id == NO))
                    .with(Prop::Cancel, Value::Bool(id == NO))
            };
            page::ensure_built(self.ui.gui_mut(), root, || {
                NodeSpec::new(NodeKind::Dialog).children([YES, NO].map(button))
            });
        }
        self.prompt = Some(prompt);
    }

    /// The answered box goes; the browser takes the input again.
    pub fn take_prompt(&mut self) -> Option<SavedSeedPrompt<I>> {
        self.ui.close(PROMPT_DIALOG);
        let prompt = self.prompt.take();
        if self.is_naming_conversion() {
            self.focus_edit();
        }
        prompt
    }

    /// The edit focus back after a warning (0x00559269).
    pub fn focus_edit(&mut self) {
        self.ui.focus_control(BROWSER_DIALOG, Some(EDIT));
    }

    /// Window focus loss: the release that would end the held press and
    /// the scroll repeat never arrives.
    pub fn reset_interaction(&mut self) {
        self.ui.gui_mut().cancel_pointer();
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
                migratable: false,
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
            .filter_map(|i| storage[i].take())
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
        let top = initial
            .unwrap_or(0)
            .min(rows.len().saturating_sub(visible_rows));
        let mut ui = DialogController::default();
        ui.push(BROWSER_DIALOG);
        if let Some(root) = ui.root_in(BROWSER_DIALOG) {
            page::ensure_built(ui.gui_mut(), root, || spec(mode));
        }
        let mut result = Self {
            mode,
            entries: rows,
            prompt: None,
            ui,
            current_description,
            conversion_source: None,
        };
        result.set_list(selected, top);
        if mode == SavedSeedMode::Save {
            result.assign_selected_description();
        }
        result
    }

    fn assign_selected_description(&mut self) {
        let text = match self.selected_entry() {
            Some(_) if !self.action_enabled() => SeedDescription::default(),
            Some(row) if row.file_name.is_some() => row.description.clone(),
            _ => self.current_description.clone(),
        };
        self.set_description(&text);
    }

    /// A row the list selected; Save shows its description in the edit.
    pub fn select(&mut self, index: usize) {
        if index >= self.entries.len() {
            return;
        }
        self.ui.set_prop(
            BROWSER_DIALOG,
            LIST,
            Prop::Selected,
            Value::Int(index as i32),
        );
        if self.mode == SavedSeedMode::Save {
            self.assign_selected_description();
        }
    }

    /// Select an output by its file identity after rebuilding the browser.
    pub fn select_file(&mut self, file: &I, visible_rows: usize) -> bool {
        let Some(index) = self
            .entries
            .iter()
            .position(|row| row.file_name.as_ref() == Some(file))
        else {
            return false;
        };
        self.reveal_row(index, visible_rows);
        true
    }

    fn reveal_row(&mut self, index: usize, visible_rows: usize) {
        let visible_rows = visible_rows.max(1);
        let top = self
            .top()
            .clamp((index + 1).saturating_sub(visible_rows), index)
            .min(self.entries.len().saturating_sub(visible_rows));
        self.ui
            .set_prop(BROWSER_DIALOG, LIST, Prop::Top, Value::Int(top as i32));
        self.select(index);
    }

    /// Use the normal Save browser to choose a destination. Even a file with a
    /// future timestamp ahead of the new slot must require an explicit selection.
    pub fn set_conversion_source(&mut self, source: I, visible_rows: usize) {
        self.conversion_source = Some(source);
        for (id, key) in [
            (LIST, "VERA:SaveConversionSlotHelp"),
            (EDIT, "VERA:SaveConversionNameHelp"),
            (ACTION, "VERA:SaveConversionSaveHelp"),
            (BACK, "VERA:SaveConversionBackHelp"),
        ] {
            self.ui
                .set_prop(BROWSER_DIALOG, id, Prop::Tooltip, Value::Text(key.into()));
        }
        if let Some(index) = self.entries.iter().position(|row| row.file_name.is_none()) {
            self.reveal_row(index, visible_rows);
        }
    }

    pub fn action_enabled(&self) -> bool {
        //558DD0 enables from visible count, even when Load has no selection
        // or Save has a blank description. The action loop handles those cases.
        if let Some(source) = self.conversion_source() {
            return self.selected_entry().is_some_and(|row| {
                row.file_name
                    .as_ref()
                    .is_none_or(|file| file != source && row.loadable)
            });
        }
        !self.entries.is_empty()
    }

    pub fn action_outcome(&self) -> Option<SavedSeedOutcome<I>> {
        let entry = self.selected_entry()?;
        match self.mode {
            SavedSeedMode::Save if !self.action_enabled() => None,
            SavedSeedMode::Save => Some(SavedSeedOutcome::Save {
                file_name: entry.file_name.clone(),
                description: self.description(),
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

    /// VERA's Setups page: what its Load (`LOAD`) or Delete (`DELETE`) does.
    pub fn setup_outcome(&self, control: u16) -> Option<SavedSeedOutcome<I>> {
        let saved = self.selected_saved()?.clone();
        match control {
            LOAD => Some(SavedSeedOutcome::Load(saved)),
            DELETE => Some(SavedSeedOutcome::Delete(saved)),
            _ => None,
        }
    }

    pub fn remove_entry(&mut self, file_name: &I, visible_rows: usize) {
        self.entries
            .retain(|row| row.file_name.as_ref() != Some(file_name));
        let top = self
            .top()
            .min(self.entries.len().saturating_sub(visible_rows));
        self.set_list((!self.entries.is_empty()).then_some(0), top);
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;

    fn seed(name: &str, time: u64, valid: bool) -> SavedSeed {
        SavedSeed {
            file_name: SavedSeedFile::in_random_maps(format!("{name}.SED").into()),
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
            Some(&SavedSeedFile::in_random_maps("Future.SED".into()))
        );
        assert_eq!(state.description(), "Future");
        state.select(1);
        assert_eq!(state.description(), "Working");
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
            Some(&SavedSeedFile::in_random_maps("B.SED".into()))
        );
        let no_selection = open(
            SavedSeedMode::Load,
            vec![seed("Invalid", 30, false), seed("A", 20, true)],
        );
        assert_eq!(no_selection.selected(), None);
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
        assert_eq!(
            ties.entries[0].file_name.as_ref(),
            Some(&SavedSeedFile::in_random_maps("A.SED".into()))
        );
    }

    #[test]
    fn blank_save_is_enabled_and_requests_validation_and_assignment_is_bounded() {
        let mut state = open(SavedSeedMode::Save, vec![]);
        state.set_description(&"   ".into());
        assert!(state.action_enabled());
        assert_eq!(
            state.action_outcome(),
            Some(SavedSeedOutcome::Save {
                file_name: None,
                description: SeedDescription::default()
            })
        );
        state.set_description(&"x".repeat(100).into());
        assert_eq!(state.edit().units.len(), 79);
    }

    #[test]
    fn path_identity_survives_selection_save_and_delete_without_text_conversion() {
        let path = std::path::PathBuf::from("saves/保存 — ridge.bin");
        let row = SavedSeedBrowserRow {
            file_name: Some(path.clone()),
            description: "Saved description".into(),
            last_write_time: 110,
            visible: true,
            loadable: true,
            migratable: false,
        };
        let mut state = SavedSeedBrowserState::open_rows(
            SavedSeedMode::Save,
            vec![row],
            "Working".into(),
            "[EMPTY SLOT]".into(),
            100,
            2,
        );
        assert_eq!(
            state.selected_entry().unwrap().file_name.as_ref(),
            Some(&path)
        );
        state.set_description(&"Changed description".into());
        assert_eq!(
            state.action_outcome(),
            Some(SavedSeedOutcome::Save {
                file_name: Some(path.clone()),
                description: "Changed description".into()
            })
        );
        state.remove_entry(&path, 2);
        assert_eq!(state.entries.len(), 1);
        assert_eq!(state.entries[0].file_name, None);
        state.select(0);
        assert_eq!(state.description(), "Working");
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
        state.remove_entry(&SavedSeedFile::in_random_maps("B.SED".into()), 2);
        assert_eq!(state.selected(), Some(0));
        state.remove_entry(&SavedSeedFile::in_random_maps("A.SED".into()), 2);
        state.remove_entry(&SavedSeedFile::in_random_maps("C.SED".into()), 2);
        assert!(state.entries.is_empty());
        assert!(!state.action_enabled());
    }
}
