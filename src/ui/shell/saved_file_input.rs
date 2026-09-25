//! Shared input for the native saved-file browser (driver 558DD0).
//! Persistence, prompt purpose, audio and font measurement remain app-owned.

use crate::ui::skirmish_shell::seed_list::SeedListGeometry;
use crate::ui::skirmish_shell::{
    SavedSeedBrowserState, SavedSeedControl, SavedSeedLayout, SavedSeedMode, SavedSeedOutcome,
};
use std::time::{Duration, Instant};
use winit::keyboard::KeyCode;

pub enum BrowserInputResult<I> {
    None,
    Outcome(SavedSeedOutcome<I>),
    PromptAnswer(bool),
    ButtonPressed,
}

/// Native list notifications use the host's double-click policy.
#[cfg(windows)]
pub fn host_double_click_limits() -> (Duration, i32, i32) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetDoubleClickTime() -> u32;
        fn GetSystemMetrics(index: i32) -> i32;
    }
    // SAFETY: pointer-free Win32 queries with no ownership effects.
    unsafe {
        (
            Duration::from_millis(u64::from(GetDoubleClickTime())),
            GetSystemMetrics(36),
            GetSystemMetrics(37),
        )
    }
}
#[cfg(not(windows))]
pub fn host_double_click_limits() -> (Duration, i32, i32) {
    (Duration::from_millis(500), 4, 4)
}

pub fn key<I: Clone + PartialEq>(
    browser: &mut SavedSeedBrowserState<I>,
    code: Option<KeyCode>,
    text: Option<&str>,
) -> BrowserInputResult<I> {
    if browser.prompt.is_some() {
        // 5D4DDB IsDialogMessageA / 5D370D maps initial-focus IDOK and
        // IDCANCEL to result 1 (negative for confirms). Consume the key.
        return if matches!(
            code,
            Some(KeyCode::Escape | KeyCode::Enter | KeyCode::NumpadEnter)
        ) {
            BrowserInputResult::PromptAnswer(false)
        } else {
            BrowserInputResult::None
        };
    }
    if !browser.description_edit.focused {
        return BrowserInputResult::None;
    }
    if matches!(code, Some(KeyCode::Enter | KeyCode::NumpadEnter)) {
        return browser
            .action_outcome()
            .map_or(BrowserInputResult::None, BrowserInputResult::Outcome);
    }
    let edit = &mut browser.description_edit;
    match code {
        Some(KeyCode::Backspace) => edit.backspace(),
        Some(KeyCode::Delete) => edit.delete(),
        Some(KeyCode::ArrowLeft) => edit.left(),
        Some(KeyCode::ArrowRight) => edit.right(),
        Some(KeyCode::Home) => edit.home(),
        Some(KeyCode::End) => edit.end(),
        // Base-browser callbacks ignore IDCANCEL; Escape does not close it.
        Some(KeyCode::Tab | KeyCode::Escape) => {}
        _ => {
            if let Some(text) = text {
                edit.insert_text(text);
            }
        }
    }
    BrowserInputResult::None
}

fn hit<I>(
    browser: &SavedSeedBrowserState<I>,
    layout: &SavedSeedLayout,
    extent: (u32, u32),
    pointer: (i32, i32),
) -> Option<SavedSeedControl> {
    let (x, y) = pointer;
    if let Some(prompt) = browser.prompt.as_ref() {
        prompt.control_at(extent.0, extent.1, x, y)
    } else {
        SeedListGeometry::new(layout.list, browser.entries.len(), browser.top_index)
            .scroll_control_at(x, y)
            .or_else(|| crate::ui::skirmish_shell::saved_seed_control_at(layout, x, y))
    }
}

pub fn mouse_down<I: Clone + PartialEq>(
    browser: &mut SavedSeedBrowserState<I>,
    layout: &SavedSeedLayout,
    extent: (u32, u32),
    pointer: (i32, i32),
    now: Instant,
    double_click_limits: (Duration, i32, i32),
) -> BrowserInputResult<I> {
    let (x, y) = pointer;
    let hit = hit(browser, layout, extent, pointer);
    browser.pressed_control = None;
    if browser.prompt.is_some() {
        browser.pressed_control = hit;
        return BrowserInputResult::None;
    }
    browser.description_edit.focused = hit == Some(SavedSeedControl::NameEdit0x526);
    match hit {
        Some(SavedSeedControl::List) => {
            if let Some(row) = SeedListGeometry::new(
                layout.list,
                browser.entries.len(),
                browser.top_index,
            )
            .row_at(browser.entries.len(), browser.top_index, x, y)
            {
                browser.select(row);
                let (time, width, height) = double_click_limits;
                let double_click = browser.last_list_press.is_some_and(|(last, px, py)| {
                    now.duration_since(last) <= time
                        && (x - px).abs() * 2 <= width
                        && (y - py).abs() * 2 <= height
                });
                browser.last_list_press = if double_click {
                    None
                } else {
                    Some((now, x, y))
                };
                if double_click && browser.mode == SavedSeedMode::Load {
                    return browser
                        .action_outcome()
                        .map_or(BrowserInputResult::None, BrowserInputResult::Outcome);
                }
            }
        }
        Some(SavedSeedControl::ScrollUp | SavedSeedControl::ScrollDown) => {
            let geometry =
                SeedListGeometry::new(layout.list, browser.entries.len(), browser.top_index);
            if hit == Some(SavedSeedControl::ScrollUp) {
                browser.top_index = browser.top_index.saturating_sub(1);
            } else {
                browser.top_index = (browser.top_index + 1).min(geometry.max_top);
            }
            browser.pressed_control = hit;
            browser.scroll_repeat_at = Some(now + Duration::from_millis(500));
        }
        Some(SavedSeedControl::ScrollThumb) => browser.pressed_control = hit,
        Some(SavedSeedControl::ScrollTrack) => {
            browser.top_index =
                SeedListGeometry::new(layout.list, browser.entries.len(), browser.top_index)
                    .top_at_pointer(y);
        }
        // A disabled window takes no press (Load with no rows, 0x00558FE5).
        Some(SavedSeedControl::Action) if !browser.action_enabled() => {}
        Some(SavedSeedControl::Action | SavedSeedControl::Back0x686) => {
            browser.pressed_control = hit;
            return BrowserInputResult::ButtonPressed;
        }
        _ => {}
    }
    BrowserInputResult::None
}

pub fn mouse_up<I: Clone + PartialEq>(
    browser: &mut SavedSeedBrowserState<I>,
    layout: &SavedSeedLayout,
    extent: (u32, u32),
    pointer: (i32, i32),
) -> BrowserInputResult<I> {
    let hit = hit(browser, layout, extent, pointer);
    let pressed = browser.pressed_control.take();
    browser.scroll_repeat_at = None;
    if pressed == Some(SavedSeedControl::ScrollThumb) {
        browser.top_index =
            SeedListGeometry::new(layout.list, browser.entries.len(), browser.top_index)
                .top_at_pointer(pointer.1);
        return BrowserInputResult::None;
    }
    if pressed.is_none() || pressed != hit {
        return BrowserInputResult::None;
    }
    if browser.prompt.is_some() {
        return BrowserInputResult::PromptAnswer(hit == Some(SavedSeedControl::Action));
    }
    match hit {
        Some(SavedSeedControl::Action) => browser
            .action_outcome()
            .map_or(BrowserInputResult::None, BrowserInputResult::Outcome),
        Some(SavedSeedControl::Back0x686) => BrowserInputResult::Outcome(SavedSeedOutcome::Close),
        _ => BrowserInputResult::None,
    }
}

/// Returns whether the app should also update its font-measured edit scroll.
pub fn update_scroll<I>(
    browser: &mut SavedSeedBrowserState<I>,
    layout: &SavedSeedLayout,
    pointer_y: i32,
    pointer_moved: bool,
    now: Instant,
) -> bool {
    if browser.prompt.is_some() {
        return false;
    }
    let geometry = SeedListGeometry::new(layout.list, browser.entries.len(), browser.top_index);
    if pointer_moved && browser.pressed_control == Some(SavedSeedControl::ScrollThumb) {
        browser.top_index = geometry.top_at_pointer(pointer_y);
    }
    if browser
        .scroll_repeat_at
        .is_some_and(|deadline| now >= deadline)
    {
        match browser.pressed_control {
            Some(SavedSeedControl::ScrollUp) => {
                browser.top_index = browser.top_index.saturating_sub(1)
            }
            Some(SavedSeedControl::ScrollDown) => {
                browser.top_index = (browser.top_index + 1).min(geometry.max_top)
            }
            _ => {
                browser.scroll_repeat_at = None;
                return false;
            }
        }
        browser.scroll_repeat_at = Some(now + Duration::from_millis(25));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::skirmish_shell::{SavedSeedBrowserRow, SavedSeedPrompt, SavedSeedPromptPurpose};
    use std::path::PathBuf;

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
            })
            .collect();
        let browser = SavedSeedBrowserState::open_rows(
            mode,
            rows,
            "Working".into(),
            "New".into(),
            100,
            SeedListGeometry::new(layout.list, count, 0).visible_rows,
        );
        (browser, layout)
    }

    #[test]
    fn prompt_blocks_editor_and_base_escape_does_not_close_browser() {
        let (mut browser, _) = fixture(SavedSeedMode::Save, 0);
        let before = browser.description_edit.units.clone();
        assert!(matches!(
            key(&mut browser, Some(KeyCode::Escape), None),
            BrowserInputResult::None
        ));
        assert_eq!(browser.description_edit.units, before);
        browser.prompt = Some(SavedSeedPrompt {
            purpose: SavedSeedPromptPurpose::Error,
            body: "Failure".into(),
            affirmative: "OK".into(),
            negative: None,
        });
        assert!(matches!(
            key(&mut browser, None, Some("typed")),
            BrowserInputResult::None
        ));
        assert_eq!(browser.description_edit.units, before);
        assert!(matches!(
            key(&mut browser, Some(KeyCode::Enter), None),
            BrowserInputResult::PromptAnswer(false)
        ));
        assert!(browser.prompt.is_some(), "the app owns prompt resolution");
    }

    #[test]
    fn double_click_load_fires_on_second_press_without_release_replay() {
        let (mut browser, layout) = fixture(SavedSeedMode::Load, 2);
        let row = SeedListGeometry::new(layout.list, 2, 0).row(1);
        let pointer = (row.x + 2, row.y + 2);
        let now = Instant::now();
        let limits = (Duration::from_millis(500), 4, 4);
        assert!(matches!(
            mouse_down(&mut browser, &layout, (800, 600), pointer, now, limits),
            BrowserInputResult::None
        ));
        assert!(matches!(
            mouse_up(&mut browser, &layout, (800, 600), pointer),
            BrowserInputResult::None
        ));
        assert!(
            matches!(mouse_down(&mut browser, &layout, (800,600), pointer, now+Duration::from_millis(100), limits), BrowserInputResult::Outcome(SavedSeedOutcome::Load(path)) if path == PathBuf::from("save1.bin"))
        );
        assert!(matches!(
            mouse_up(&mut browser, &layout, (800, 600), pointer),
            BrowserInputResult::None
        ));
    }

    #[test]
    fn a_disabled_load_takes_no_press() {
        let (mut browser, layout) = fixture(SavedSeedMode::Load, 0);
        assert!(!browser.action_enabled());
        let pointer = (layout.action.x + 5, layout.action.y + 5);
        let limits = (Duration::from_millis(500), 4, 4);
        assert!(matches!(
            mouse_down(
                &mut browser,
                &layout,
                (800, 600),
                pointer,
                Instant::now(),
                limits
            ),
            BrowserInputResult::None
        ));
        assert_eq!(browser.pressed_control, None);
    }

    #[test]
    fn scrollbar_hold_repeats_at_deadline_and_release_cancels_capture() {
        let (mut browser, layout) = fixture(SavedSeedMode::Load, 40);
        let bar = SeedListGeometry::new(layout.list, 40, 0).scrollbar.unwrap();
        let pointer = (bar.x + 3, bar.y + bar.h - 3);
        let now = Instant::now();
        let _ = mouse_down(
            &mut browser,
            &layout,
            (800, 600),
            pointer,
            now,
            (Duration::from_millis(500), 4, 4),
        );
        assert_eq!(browser.top_index, 1);
        update_scroll(
            &mut browser,
            &layout,
            pointer.1,
            false,
            now + Duration::from_millis(499),
        );
        assert_eq!(browser.top_index, 1);
        update_scroll(
            &mut browser,
            &layout,
            pointer.1,
            false,
            now + Duration::from_millis(500),
        );
        assert_eq!(browser.top_index, 2);
        assert!(matches!(
            mouse_up(&mut browser, &layout, (800, 600), (0, 0)),
            BrowserInputResult::None
        ));
        assert!(browser.pressed_control.is_none());
        assert!(browser.scroll_repeat_at.is_none());
        update_scroll(
            &mut browser,
            &layout,
            pointer.1,
            false,
            now + Duration::from_secs(1),
        );
        assert_eq!(browser.top_index, 2);
    }
}
