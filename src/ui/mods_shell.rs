//! VERA's Mods page, opened by the Mods button of Options; gamemd has none.
//! A family page shaped like the movie list `0x129`: the mods in the `Mods`
//! folder with the original game first, what the selected one says of itself,
//! and Use This Mod, Open Mods Folder and Back (docs/modding.md).
//!
//! The rows and the choice belong to `app::mods`; this is the page's shape.

use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{RectPx, SHELL_BASE_H, SHELL_BASE_W, ScaledRects, ShellScale};
use crate::ui::shell::list::ROW_HEIGHT;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};

/// Not a retail dialog: their ids stop near `0xBC7`.
pub(crate) const DIALOG: DialogId = DialogId(0x7E02);
/// The list of mods: a hover target for status help, as `0x744` is.
pub(crate) const LIST: u16 = 0x7E30;
pub(crate) const USE: u16 = 0x7E31;
pub(crate) const OPEN_FOLDER: u16 = 0x7E32;
pub(crate) const BACK: u16 = 0x7E33;

/// The Options button's caption and the page's heading.
pub(crate) const TITLE_KEY: &str = "VERA:Mods";
pub(crate) const BUTTON_HELP_KEY: &str = "VERA:ModsHelp";
/// The caption over the list, and the list's status help.
pub(crate) const PROMPT_KEY: &str = "VERA:ModsPrompt";

/// Use This Mod and Open Mods Folder on the column's first rungs, Back on the
/// bottom row.
pub(crate) const PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DIALOG,
    title_key: TITLE_KEY,
    stacked: &[
        MenuPageButtonSpec {
            id: USE,
            dlu_top: 122,
            csf_key: "VERA:ModUse",
            tooltip_key: "VERA:ModUseHelp",
            result: None,
        },
        MenuPageButtonSpec {
            id: OPEN_FOLDER,
            dlu_top: 149,
            csf_key: "VERA:ModsOpenFolder",
            tooltip_key: "VERA:ModsOpenFolderHelp",
            result: None,
        },
    ],
    back: MenuPageButtonSpec {
        id: BACK,
        dlu_top: 346,
        csf_key: "GUI:Back",
        tooltip_key: "STT:MsnDltButtonBack",
        result: Some(-1),
    },
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

/// The responsive variant of [`compute_layout`]: the 800x600 page scaled to
/// fit the window, keeping its shape. Rows grow with the scale, so the list
/// shows the rows it shows at 800x600.
pub(crate) fn compute_responsive_layout(screen_w: u32, screen_h: u32) -> ModsLayout {
    let scale = ShellScale::from_screen(screen_w as i32, screen_h as i32);
    let base = compute_layout(SHELL_BASE_W as u32, SHELL_BASE_H as u32);
    ModsLayout {
        page: menu_page::compute_responsive_layout(&PAGE, screen_w, screen_h),
        prompt: base.prompt.scaled_rects(scale),
        list: base.list.scaled_rects(scale),
        details: base.details.scaled_rects(scale),
        name: base.name.scaled_rects(scale),
        state: base.state.scaled_rects(scale),
        author: base.author.scaled_rects(scale),
        about: base.about.scaled_rects(scale),
        terms: base.terms.scaled_rects(scale),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::shell::list::{ListBoxState, ListMetrics};

    /// The page's own parts stay left of the right panel and above the lower
    /// strip at both retail sizes, one under the other, and the list shows
    /// whole rows.
    #[test]
    fn the_list_and_the_details_fit_beside_the_panel_at_both_retail_sizes() {
        for (size, rows) in [((800, 600), 9), ((640, 480), 7)] {
            let layout = compute_layout(size.0, size.1);
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
            let list = ListBoxState::new(vec![(); 30], None);
            assert_eq!(
                list.geometry(layout.list, ListMetrics::RETAIL).visible_rows,
                rows
            );
        }
        assert_eq!(
            compute_layout(800, 600).page.button_rect(OPEN_FOLDER),
            Some(RectPx::new(644, 241, 156, 42))
        );
    }

    /// A fitted window shows the nine rows 800x600 shows, at the sizes whose
    /// rounded row height once cost the list its last row.
    #[test]
    fn a_fitted_window_keeps_the_lists_rows() {
        for (width, height) in [(640, 480), (1280, 720), (1366, 768), (1920, 1080)] {
            let layout = compute_responsive_layout(width, height);
            let list = ListBoxState::new(vec![(); 30], None);
            let metrics = ListMetrics::scaled(layout.page.scale);
            assert_eq!(
                list.geometry(layout.list, metrics).visible_rows,
                9,
                "{width}x{height}"
            );
        }
    }
}
