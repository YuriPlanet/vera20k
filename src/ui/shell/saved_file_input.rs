//! Shared input for the native saved-file browser (driver 558DD0) on its
//! `vera20k-gui` widgets. Persistence, prompt purpose, audio and font
//! measurement remain app-owned.

use crate::ui::shell::page::{self, PageCue, PageEvent};
use crate::ui::skirmish_shell::saved_seed_browser::{
    ACTION, BACK, BROWSER_DIALOG, CONVERT, DELETE, EDIT, LIST, LOAD, NO, PROMPT_DIALOG, YES,
};
use crate::ui::skirmish_shell::{
    SavedSeedBrowserState, SavedSeedLayout, SavedSeedMode, SavedSeedOutcome,
};
use vera20k_gui::{InputEvent, Key, MouseButton, Prop, Value};
use winit::keyboard::KeyCode;

pub enum BrowserInputResult<I> {
    Outcome(SavedSeedOutcome<I>),
    PromptAnswer(bool),
    /// A press on an owner-draw button, a box's included (the message box
    /// `0x005D3490` subclasses its buttons to `0x00612B70`, which plays
    /// GUIMainButtonSound, `0x0061374B`), or a row selected by a press (the
    /// list subclass's GenericClick, `0x0061AA43..0x0061AA5A`).
    Cue(PageCue),
}

/// Puts the widgets on `layout` (a control without a rect is hidden) with
/// the enable rules, and the box's buttons on its template in `extent`.
fn sync<I: Clone + PartialEq>(
    browser: &mut SavedSeedBrowserState<I>,
    layout: &SavedSeedLayout,
    extent: (u32, u32),
) {
    // A disabled window takes no press: the action with no rows
    // (0x00558FE5), VERA's Setups Load and Delete without a saved row.
    let enabled = [
        (ACTION, browser.action_enabled()),
        (CONVERT, browser.convert_enabled()),
        (LOAD, browser.selected_saved().is_some()),
        (DELETE, browser.selected_saved().is_some()),
    ];
    let prompt = browser
        .prompt
        .as_ref()
        .map(|prompt| prompt.layout(extent.0, extent.1));
    let rows = Value::Int(browser.entries.len() as i32);
    let ui = &mut browser.ui;
    ui.set_scale_in(BROWSER_DIALOG, layout.scale);
    ui.set_prop(BROWSER_DIALOG, LIST, Prop::RowCount, rows);
    for (id, on) in enabled {
        ui.set_prop(BROWSER_DIALOG, id, Prop::Enabled, Value::Bool(on));
    }
    if let Some(root) = ui.root_in(BROWSER_DIALOG) {
        page::sync_rects(ui.gui_mut(), root, |id| match id {
            LIST => Some(layout.list),
            EDIT => layout.name_edit,
            ACTION => Some(layout.action),
            CONVERT => layout.convert,
            LOAD => layout.load,
            DELETE => layout.delete,
            BACK => Some(layout.back),
            _ => None,
        });
    }
    if let (Some((_, _, yes, no)), Some(root)) = (prompt, ui.root_in(PROMPT_DIALOG)) {
        page::sync_rects(ui.gui_mut(), root, |id| match id {
            YES => Some(yes),
            NO => no,
            _ => None,
        });
    }
}

/// What the widgets' events ask of the app, in order.
fn take_events<I: Clone + PartialEq>(
    browser: &mut SavedSeedBrowserState<I>,
    setups: bool,
) -> Vec<BrowserInputResult<I>> {
    let events = browser.ui.drain_events();
    let events = page::page_events(browser.ui.gui(), events);
    let mut out = Vec::new();
    for event in events {
        let result = match event {
            PageEvent::Cue(cue) => Some(BrowserInputResult::Cue(cue)),
            PageEvent::Activated(id @ (YES | NO)) if browser.prompt.is_some() => {
                Some(BrowserInputResult::PromptAnswer(id == YES))
            }
            PageEvent::Selected(LIST, row) => {
                browser.select(row as usize);
                None
            }
            // LBN_DBLCLK: the Load proc loads when the list has rows
            // (0x00558AC0); VERA's Setups page loads the selected setup.
            PageEvent::DoubleClicked(LIST, _)
                if browser.mode == SavedSeedMode::Load && browser.action_enabled() =>
            {
                browser
                    .selected_saved()
                    .cloned()
                    .map(SavedSeedOutcome::Load)
                    .map(BrowserInputResult::Outcome)
            }
            PageEvent::DoubleClicked(LIST, _) if setups => {
                browser.setup_outcome(LOAD).map(BrowserInputResult::Outcome)
            }
            PageEvent::Activated(ACTION) => {
                browser.action_outcome().map(BrowserInputResult::Outcome)
            }
            PageEvent::Activated(CONVERT) if browser.convert_enabled() => browser
                .selected_saved()
                .cloned()
                .map(SavedSeedOutcome::BeginConvert)
                .map(BrowserInputResult::Outcome),
            PageEvent::Submitted(EDIT, _) if browser.mode == SavedSeedMode::Save => {
                browser.action_outcome().map(BrowserInputResult::Outcome)
            }
            PageEvent::Activated(BACK) => {
                Some(BrowserInputResult::Outcome(SavedSeedOutcome::Close))
            }
            PageEvent::Activated(id @ (LOAD | DELETE)) => {
                browser.setup_outcome(id).map(BrowserInputResult::Outcome)
            }
            _ => None,
        };
        out.extend(result);
    }
    out
}

/// One pointer message at `pointer` (`down`: `None` moves). The window
/// system moves the pointer before a press or release; a press anywhere but
/// on the description edit takes its focus.
pub fn pointer<I: Clone + PartialEq>(
    browser: &mut SavedSeedBrowserState<I>,
    layout: &SavedSeedLayout,
    extent: (u32, u32),
    (x, y): (i32, i32),
    down: Option<bool>,
) -> Vec<BrowserInputResult<I>> {
    sync(browser, layout, extent);
    browser.ui.on_input(InputEvent::PointerMove { x, y });
    let button = MouseButton::Left;
    match down {
        Some(true) => {
            if browser.prompt.is_none() && browser.ui.gui().hovered() != browser.node(EDIT) {
                browser.ui.focus_control(BROWSER_DIALOG, None);
            }
            browser
                .ui
                .on_input(InputEvent::PointerDown { x, y, button });
        }
        Some(false) => {
            browser.ui.on_input(InputEvent::PointerUp { x, y, button });
        }
        None => {}
    }
    take_events(browser, layout.load.is_some())
}

/// A key, with the text it types, on the browser.
pub fn key<I: Clone + PartialEq>(
    browser: &mut SavedSeedBrowserState<I>,
    code: Option<KeyCode>,
    text: Option<&str>,
) -> Vec<BrowserInputResult<I>> {
    // A prompt's No takes Enter and Escape and the box every other key.
    // Enter in the edit acts; the base browser ignores IDCANCEL, so Escape
    // does not close it, and control characters (Tab too) type nothing.
    if browser.is_naming_conversion() && browser.prompt.is_none() && code == Some(KeyCode::Escape) {
        return vec![BrowserInputResult::Outcome(SavedSeedOutcome::Close)];
    }
    let event = super::controller::gui_input(code, false, text);
    let Some(event) = event.filter(|e| !matches!(e, InputEvent::Key(Key::Tab | Key::BackTab)))
    else {
        return Vec::new();
    };
    browser.ui.on_input(event);
    take_events(browser, false)
}

/// Each frame: the scrollbar arrow's repeat (`0x0061C690`), and NewEdit
/// keeps the edit's caret in view (`measure`: text widths in the font's
/// authored units).
pub fn update<I: Clone + PartialEq>(
    browser: &mut SavedSeedBrowserState<I>,
    layout: &SavedSeedLayout,
    extent: (u32, u32),
    measure: impl Fn(&str) -> u32,
) {
    sync(browser, layout, extent);
    browser.ui.poll();
    browser.ui.drain_events();
    if let (Some(_), Some(node)) = (layout.name_edit, browser.node(EDIT)) {
        let _ = browser.ui.gui_mut().keep_caret_in_view(node, &measure);
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::ui::skirmish_shell::{SavedSeedBrowserRow, SavedSeedPrompt, SavedSeedPromptPurpose};
    use std::path::PathBuf;
    use vera20k_gui::shell::list::{ListMetrics, ShellListGeometry};

    fn fixture(
        mode: SavedSeedMode,
        count: usize,
    ) -> (SavedSeedBrowserState<PathBuf>, SavedSeedLayout) {
        let layout = crate::ui::skirmish_shell::compute_saved_seed_layout(mode, 800, 600);
        let rows = (0..count)
            .map(|i| SavedSeedBrowserRow {
                file_name: Some(PathBuf::from(format!("save{i}.bin"))),
                description: format!("Save {i}").into(),
                last_write_time: (count - i) as u64,
                visible: true,
                loadable: true,
                migratable: false,
            })
            .collect();
        let visible =
            ShellListGeometry::new(layout.list, count, 0, ListMetrics::scaled(layout.scale))
                .visible_rows;
        let browser = SavedSeedBrowserState::open_rows(
            mode,
            rows,
            "Working".into(),
            "New".into(),
            100,
            visible,
        );
        (browser, layout)
    }

    fn click<I: Clone + PartialEq>(
        browser: &mut SavedSeedBrowserState<I>,
        layout: &SavedSeedLayout,
        at: (i32, i32),
    ) -> Vec<BrowserInputResult<I>> {
        let mut out = pointer(browser, layout, (800, 600), at, Some(true));
        out.extend(pointer(browser, layout, (800, 600), at, Some(false)));
        out
    }

    fn error() -> SavedSeedPrompt<PathBuf> {
        SavedSeedPrompt {
            purpose: SavedSeedPromptPurpose::Error,
            body: "Failure".into(),
            affirmative: "OK".into(),
            negative: None,
        }
    }

    #[test]
    fn prompt_blocks_editor_and_base_escape_does_not_close_browser() {
        let (mut browser, _) = fixture(SavedSeedMode::Save, 0);
        let before = browser.description();
        assert!(key(&mut browser, Some(KeyCode::Escape), Some("\u{1b}")).is_empty());
        assert_eq!(browser.description(), before);
        browser.show_prompt(error());
        assert!(key(&mut browser, None, Some("typed")).is_empty());
        assert_eq!(browser.description(), before);
        assert!(matches!(
            key(&mut browser, Some(KeyCode::Enter), None)[..],
            [BrowserInputResult::PromptAnswer(false)]
        ));
        assert!(browser.prompt.is_some(), "the app owns prompt resolution");
    }

    #[test]
    fn a_press_on_a_prompt_button_cues_and_its_release_answers() {
        let (mut browser, layout) = fixture(SavedSeedMode::Save, 0);
        let (dialog, _, ok, _) = error().layout(800, 600);
        browser.show_prompt(error());
        assert!(click(&mut browser, &layout, (dialog.x + 2, dialog.y + 2)).is_empty());
        assert!(matches!(
            click(&mut browser, &layout, (ok.x + 2, ok.y + 2))[..],
            [
                BrowserInputResult::Cue(PageCue::Button),
                BrowserInputResult::PromptAnswer(true)
            ]
        ));
    }

    #[test]
    fn double_click_loads_the_selection_even_below_the_rows() {
        let (mut browser, layout) = fixture(SavedSeedMode::Load, 2);
        let geometry = ShellListGeometry::new(layout.list, 2, 0, ListMetrics::scaled(layout.scale));
        let row = geometry.row(1);
        assert!(matches!(
            click(&mut browser, &layout, (row.x + 2, row.y + 2))[..],
            [BrowserInputResult::Cue(PageCue::ListClick)]
        ));
        assert!(matches!(
            &click(&mut browser, &layout, (row.x + 2, row.y + 2))[..],
            [BrowserInputResult::Outcome(SavedSeedOutcome::Load(path))] if *path == *"save1.bin"
        ));
        let below = geometry.row(geometry.visible_rows - 1);
        browser.reset_interaction();
        assert!(click(&mut browser, &layout, (below.x + 2, below.y + 2)).is_empty());
        assert!(matches!(
            &click(&mut browser, &layout, (below.x + 2, below.y + 2))[..],
            [BrowserInputResult::Outcome(SavedSeedOutcome::Load(path))] if *path == *"save1.bin"
        ));
        assert_eq!(browser.selected(), Some(1));
    }

    #[test]
    fn load_and_double_click_keep_their_action_on_convertible_rows() {
        let (mut browser, _) = fixture(SavedSeedMode::Load, 2);
        let layout = crate::ui::shell::saved_games::main_menu_saved_game_layout(
            SavedSeedMode::Load,
            800,
            600,
        )
        .browser;
        let action = (layout.action.x + 5, layout.action.y + 5);
        let rect = layout.convert.unwrap();
        let convert = (rect.x + 5, rect.y + 5);
        assert!(click(&mut browser, &layout, convert).is_empty());
        assert!(matches!(
            &click(&mut browser, &layout, action)[..],
            [BrowserInputResult::Cue(PageCue::Button), BrowserInputResult::Outcome(SavedSeedOutcome::Load(path))]
                if *path == *"save0.bin"
        ));
        browser.entries[1].loadable = false;
        browser.entries[1].migratable = true;
        let geometry = ShellListGeometry::new(layout.list, 2, 0, ListMetrics::scaled(layout.scale));
        let row = geometry.row(1);
        let old_save = (row.x + 2, row.y + 2);
        click(&mut browser, &layout, old_save);
        assert_eq!(
            browser.ui.prop(BROWSER_DIALOG, ACTION, Prop::Label),
            Some(&Value::Text("GUI:Load".into()))
        );
        assert!(key(&mut browser, Some(KeyCode::Enter), None).is_empty());
        assert!(matches!(
            &click(&mut browser, &layout, old_save)[..],
            [BrowserInputResult::Outcome(SavedSeedOutcome::Load(path))] if *path == *"save1.bin"
        ));
        assert!(matches!(
            &click(&mut browser, &layout, action)[..],
            [BrowserInputResult::Cue(PageCue::Button), BrowserInputResult::Outcome(SavedSeedOutcome::Load(path))]
                if *path == *"save1.bin"
        ));
        assert!(matches!(&click(&mut browser, &layout, convert)[..],
            [BrowserInputResult::Cue(PageCue::Button), BrowserInputResult::Outcome(SavedSeedOutcome::BeginConvert(source))]
                if *source == *"save1.bin"));
        browser.entries[1].migratable = false;
        assert!(click(&mut browser, &layout, convert).is_empty());
        assert!(!browser.is_naming_conversion());
    }

    #[test]
    fn conversion_destinations_use_the_save_list_and_description_edit() {
        let (sources, _) = fixture(SavedSeedMode::Load, 3);
        let mut rows = sources.entries;
        rows[1].loadable = false;
        let source = rows[0].file_name.clone().unwrap();
        let mut browser = SavedSeedBrowserState::open_rows(
            SavedSeedMode::Save,
            rows,
            "".into(),
            "New".into(),
            0,
            8,
        );
        assert_eq!(browser.selected_saved(), Some(&source));
        browser.set_conversion_source(source.clone(), 8);
        assert_eq!(browser.conversion_source(), Some(&source));
        assert!(browser.selected_entry().unwrap().file_name.is_none());
        assert_eq!(browser.top(), 0);
        assert!(browser.edit().focused);
        let shell = crate::ui::shell::in_game_shell::allied_fixture_layout(800, 600);
        let layout = crate::ui::shell::saved_games::saved_game_layout(
            SavedSeedMode::Save,
            800,
            600,
            shell,
            [147, 44],
        );
        let at = |rect: crate::ui::shell::geom::RectPx| (rect.x + 5, rect.y + 5);
        pointer(
            &mut browser,
            &layout,
            (800, 600),
            at(layout.name_edit.unwrap()),
            None,
        );
        assert_eq!(browser.hovered_help(), Some("VERA:SaveConversionNameHelp"));
        key(&mut browser, None, Some("中文战役"));
        assert!(matches!(&key(&mut browser, Some(KeyCode::Enter), None)[..],
            [BrowserInputResult::Outcome(SavedSeedOutcome::Save {file_name: None, description})]
                if description.display_text() == "中文战役"));

        let list = ShellListGeometry::new(
            layout.list,
            4,
            browser.top(),
            ListMetrics::scaled(layout.scale),
        );
        for read_only in [0, 1] {
            click(&mut browser, &layout, at(list.row(read_only)));
            assert!(browser.description().is_empty());
            assert!(!browser.action_enabled());
            key(&mut browser, None, Some("edited"));
            assert!(key(&mut browser, Some(KeyCode::Enter), None).is_empty());
            assert!(click(&mut browser, &layout, at(layout.action)).is_empty());
        }
        click(&mut browser, &layout, at(list.row(2)));
        assert_eq!(browser.description(), "Save 2");
        key(&mut browser, None, Some("新"));
        assert!(
            matches!(&click(&mut browser, &layout, at(layout.action))[..],
            [BrowserInputResult::Cue(PageCue::Button), BrowserInputResult::Outcome(SavedSeedOutcome::Save {file_name: Some(file), description})]
                if *file == *"save2.bin" && description.display_text() == "Save 2新")
        );

        browser.show_prompt(error());
        assert!(matches!(
            &key(&mut browser, Some(KeyCode::Escape), None)[..],
            [BrowserInputResult::PromptAnswer(false)]
        ));
        browser.take_prompt();
        assert_eq!(browser.description(), "Save 2新");
        assert!(browser.edit().focused);
        assert!(matches!(
            &key(&mut browser, Some(KeyCode::Escape), None)[..],
            [BrowserInputResult::Outcome(SavedSeedOutcome::Close)]
        ));
        assert!(matches!(
            &click(&mut browser, &layout, at(layout.back))[..],
            [
                BrowserInputResult::Cue(PageCue::Button),
                BrowserInputResult::Outcome(SavedSeedOutcome::Close)
            ]
        ));
    }

    #[test]
    fn a_disabled_load_takes_no_press_and_setups_buttons_take_theirs() {
        let (mut browser, layout) = fixture(SavedSeedMode::Load, 0);
        assert!(!browser.action_enabled());
        assert!(
            click(
                &mut browser,
                &layout,
                (layout.action.x + 5, layout.action.y + 5)
            )
            .is_empty()
        );
        // At 640x480 the list reaches under the Setups column: a press right
        // of it lands on Delete.
        let layout = crate::ui::skirmish_shell::compute_setups_layout(640, 480);
        let (mut browser, _) = fixture(SavedSeedMode::Save, 1);
        browser.select(1);
        let delete = layout.delete.unwrap();
        assert!(matches!(
            &click(&mut browser, &layout, (delete.x + delete.w - 4, delete.y + 4))[..],
            [BrowserInputResult::Cue(PageCue::Button), BrowserInputResult::Outcome(SavedSeedOutcome::Delete(path))]
                if *path == *"save0.bin"
        ));
    }

    #[test]
    fn load_status_help_follows_the_cursor_again_once_the_box_goes() {
        let (mut browser, layout) = fixture(SavedSeedMode::Load, 2);
        let over = (layout.list.x + 5, layout.list.y + 5);
        pointer(&mut browser, &layout, (800, 600), over, None);
        assert_eq!(browser.hovered_help(), Some("STT:LoadList"));
        browser.show_prompt(error());
        assert_eq!(browser.hovered_help(), None);
        browser.take_prompt();
        pointer(&mut browser, &layout, (800, 600), over, None);
        assert_eq!(browser.hovered_help(), Some("STT:LoadList"));
    }

    #[test]
    fn typing_is_limited_and_a_press_off_the_edit_takes_its_focus() {
        let (mut browser, layout) = fixture(SavedSeedMode::Save, 0);
        key(&mut browser, None, Some(&"x".repeat(100)));
        assert_eq!(browser.edit().units.len(), 79);
        key(&mut browser, Some(KeyCode::Backspace), None);
        key(&mut browser, None, Some(":"));
        assert_eq!(browser.edit().units[78], u16::from(b':'));
        click(
            &mut browser,
            &layout,
            (layout.back.x - 2, layout.back.y - 2),
        );
        key(&mut browser, None, Some("y"));
        assert_eq!(browser.edit().units.len(), 79);
        assert!(!browser.edit().focused);
    }
}
