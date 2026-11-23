//! VERA's Mods page, opened by the Mods button of Options; gamemd has none.
//! A family page shaped like the movie list `0x129`: the mods in the `Mods`
//! folder with the original game first, what the selected one says of itself,
//! and Use This Mod, Check Files, New Mod, Add Mod, Open Mods Folder and Back
//! (docs/modding.md).
//!
//! The rows and the choice belong to `app::mods`; this is the page's shape.

use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use vera20k_gui::shell::list::ROW_HEIGHT;

/// Not a retail dialog: their ids stop near `0xBC7`.
pub(crate) const DIALOG: DialogResourceId = DialogResourceId(0x7E02);
pub(crate) const USE: u16 = 0x7E31;
pub(crate) const OPEN_FOLDER: u16 = 0x7E32;
pub(crate) const BACK: u16 = 0x7E33;
pub(crate) const CHECK: u16 = 0x7E34;
pub(crate) const NEW: u16 = 0x7E35;
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub(crate) const ADD: u16 = 0x7E36;

/// The Options button's caption and the page's heading.
pub(crate) const TITLE_KEY: &str = "VERA:Mods";
pub(crate) const BUTTON_HELP_KEY: &str = "VERA:ModsHelp";
/// The caption over the list, and the list's status help.
pub(crate) const PROMPT_KEY: &str = "VERA:ModsPrompt";

/// The mod actions in the right column, Back on the bottom row.
pub(crate) const PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DIALOG,
    title_key: TITLE_KEY,
    stacked: &[
        MenuPageButtonSpec::new(USE, 122, "VERA:ModUse", "VERA:ModUseHelp"),
        MenuPageButtonSpec::new(CHECK, 149, "VERA:ModCheck", "VERA:ModCheckHelp"),
        MenuPageButtonSpec::new(NEW, 176, "VERA:ModNew", "VERA:ModNewHelp"),
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        MenuPageButtonSpec::new(ADD, 203, "VERA:ModAdd", "VERA:ModAddHelp"),
        MenuPageButtonSpec::new(
            OPEN_FOLDER,
            230,
            "VERA:ModsOpenFolder",
            "VERA:ModsOpenFolderHelp",
        ),
    ],
    back: MenuPageButtonSpec::new(BACK, 346, "GUI:Back", "STT:MsnDltButtonBack").returning(-1),
};

/// Where the page's own parts go, left of the right panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModsLayout {
    pub(crate) page: MenuPageLayout,
    /// The caption over the list.
    pub(crate) prompt: RectPx,
    pub(crate) list: RectPx,
    /// The box under the list that holds the selected mod's own account.
    pub(crate) details: RectPx,
    /// Its lines, top to bottom: the name, its state (in use, or what it
    /// needs), who made it, what it is, and its terms.
    pub(crate) name: RectPx,
    pub(crate) state: RectPx,
    pub(crate) author: RectPx,
    pub(crate) about: RectPx,
    pub(crate) terms: RectPx,
}

/// The detail box's single lines stand a list row apart: GAME.FNT's 17 px
/// lines are tight for the taller letters of Chinese.
const LINE: i32 = ROW_HEIGHT;

/// Pixels under the list's last row at 800x600 (the movie list has 19).
const ROW_SLACK: i32 = 10;

pub(crate) fn compute_layout(screen_w: u32, screen_h: u32) -> ModsLayout {
    let page = menu_page::compute_layout(&PAGE, screen_w, screen_h);
    // The page left of the right panel and above the lower strip: 632x568 at
    // 800x600 and 472x448 at 640x480, the two sizes the retail layout runs at.
    let width = page.right_panel.top.x;
    let height = page.lower_strip.y;
    let x = width / 10;
    let w = width - 2 * x;
    let prompt = RectPx::new(x, height / 10, w, LINE + 1);
    // Nine rows at 800x600, seven at 640x480, and room under the last: a
    // fitted window rounds its row height, and a list that ends on its last
    // row at 800x600 loses that row at some sizes.
    let rows = (height / 63).max(1);
    let list = RectPx::new(x, prompt.y + 26, w, rows * ROW_HEIGHT + ROW_SLACK);
    let details_y = list.y + list.h + 16;
    let details = RectPx::new(x, details_y, w, (height - 20 - details_y).max(0));
    // Inside the box: one line each for the name, the state and the maker,
    // then what is left shared by the two texts that wrap.
    let inset = 8;
    let text = |y: i32, h: i32| RectPx::new(x + inset, y, w - 2 * inset, h);
    let name = text(details.y + inset, LINE);
    let state = text(name.y + LINE, LINE);
    let author = text(state.y + LINE, LINE);
    let wrapped_y = author.y + LINE + 6;
    let wrapped_h = ((details.y + details.h - inset - wrapped_y - 6) / 2).max(0);
    let about = text(wrapped_y, wrapped_h);
    let terms = text(wrapped_y + wrapped_h + 6, wrapped_h);
    ModsLayout {
        page,
        prompt,
        list,
        details,
        name,
        state,
        author,
        about,
        terms,
    }
}

vera20k_gui::scaled_rects!(ModsLayout {
    page,
    prompt,
    list,
    details,
    name,
    state,
    author,
    about,
    terms
});

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::ui::shell::geom::fit_window;
    use vera20k_gui::shell::list::{ListMetrics, list_box_geometry};

    /// The page's own parts stay left of the right panel and above the lower
    /// strip at both retail sizes, one under the other, and the list shows
    /// whole rows.
    #[test]
    fn the_list_and_the_details_fit_beside_the_panel_at_both_retail_sizes() {
        for (size, rows) in [((800, 600), 9), ((640, 480), 7)] {
            let layout = compute_layout(size.0, size.1);
            #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
            {
                let add = layout.page.button_rect(ADD).unwrap();
                let new = layout.page.button_rect(NEW).unwrap();
                let back = layout.page.button_rect(BACK).unwrap();
                assert!(
                    add.y >= new.y + new.h && add.y + add.h <= back.y,
                    "{size:?}"
                );
            }
            let right = layout.page.right_panel.top.x;
            let bottom = layout.page.lower_strip.y;
            let mut above = 0;
            for rect in [layout.prompt, layout.list, layout.details] {
                assert!(rect.x > 0 && rect.x + rect.w < right, "{size:?} {rect:?}");
                assert!(
                    rect.y >= above && rect.y + rect.h < bottom,
                    "{size:?} {rect:?}"
                );
                above = rect.y + rect.h;
            }
            let mut line = layout.details.y;
            for rect in [
                layout.name,
                layout.state,
                layout.author,
                layout.about,
                layout.terms,
            ] {
                assert!(rect.y >= line && rect.h >= LINE, "{size:?} {rect:?}");
                line = rect.y + rect.h;
            }
            assert!(line < layout.details.y + layout.details.h);
            assert_eq!(
                list_box_geometry(layout.list, 30, 0, ListMetrics::RETAIL).visible_rows,
                rows
            );
        }
        assert_eq!(
            compute_layout(800, 600).page.button_rect(OPEN_FOLDER),
            Some(RectPx::new(644, 367, 156, 42))
        );
    }

    /// A fitted window shows the nine rows 800x600 shows, at the sizes whose
    /// rounded row height once cost the list its last row.
    #[test]
    fn a_fitted_window_keeps_the_lists_rows() {
        for (width, height) in [(640, 480), (1280, 720), (1366, 768), (1920, 1080)] {
            let layout = fit_window(width, height, compute_layout);
            let metrics = ListMetrics::scaled(layout.page.scale);
            assert_eq!(
                list_box_geometry(layout.list, 30, 0, metrics).visible_rows,
                9,
                "{width}x{height}"
            );
        }
    }
}
