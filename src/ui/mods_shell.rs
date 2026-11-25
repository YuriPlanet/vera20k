//! VERA's Mods page, opened by the Mods button of Options; gamemd has none.
//! A family page shaped like the movie list `0x129`: the mods in the `Mods`
//! folder with the original game first, what the selected one says of itself,
//! and Use This Mod, Check Files, New Mod, Add Mod, Open Mods Folder and Back
//! (docs/modding.md).
//!
//! The rows and the choice belong to `app::mods`; this is the page's shape.

use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::{RectPx, ShellAlign};
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use vera20k_gui::shell::list::{ListMetrics, ROW_HEIGHT, ShellListGeometry, list_box_rect};
use vera20k_gui::{NodeKind, NodeSpec, Prop, Value};

/// Not a retail dialog: their ids stop near `0xBC7`.
pub(crate) const DIALOG: DialogResourceId = DialogResourceId(0x7E02);
pub(crate) const USE: u16 = 0x7E31;
pub(crate) const OPEN_FOLDER: u16 = 0x7E32;
pub(crate) const BACK: u16 = 0x7E33;
pub(crate) const CHECK: u16 = 0x7E34;
pub(crate) const NEW: u16 = 0x7E35;
#[cfg(any(
    windows,
    target_os = "macos",
    target_os = "linux",
    target_os = "android"
))]
pub(crate) const ADD: u16 = 0x7E36;
/// Android: remove the selected mod's folder (the app owns it).
#[cfg(target_os = "android")]
pub(crate) const DELETE: u16 = 0x7E37;
/// Android: add a home-screen shortcut for the selected mod.
#[cfg(target_os = "android")]
pub(crate) const PIN: u16 = 0x7E38;
/// Android: rename the selected mod's folder.
#[cfg(target_os = "android")]
pub(crate) const RENAME: u16 = 0x7E39;

/// The Options button's caption and the page's heading.
pub(crate) const TITLE_KEY: &str = "VERA:Mods";
pub(crate) const BUTTON_HELP_KEY: &str = "VERA:ModsHelp";
/// The caption over the list, and the list's status help.
pub(crate) const PROMPT_KEY: &str = "VERA:ModsPrompt";
/// The list's host id, past the dialog's 16-bit control ids as the movie
/// list's ([`crate::ui::movies_credits_shell::FAMILY_LIST_ID`]): the rect
/// sync leaves its window alone and no press names it; the painter finds its
/// rows by it.
pub(crate) const LIST_ID: i32 = 0x1_0001;
/// The page's own statics, VERA's: the empty folder's hint across the list,
/// the selected mod's lines in the box under it (name, state, maker, what
/// it is, terms) and the caption over the list. A static with no text
/// shows nothing.
pub(crate) const HINT_TEXT: u16 = 0x7E40;
pub(crate) const DETAIL_TEXTS: [u16; 5] = [0x7E41, 0x7E42, 0x7E43, 0x7E44, 0x7E45];
pub(crate) const PROMPT_TEXT: u16 = 0x7E46;

/// Android: New Mod's and Rename Mod's name dialog, a page dialog rather than
/// a message box so the edit keeps its text and focus across frames. Hidden
/// until one of them opens it (`app::shell_mods`).
#[cfg(target_os = "android")]
pub(crate) const NAME_EDIT: u16 = 0x7E62;
#[cfg(target_os = "android")]
pub(crate) const NAME_OK: u16 = 0x7E63;
#[cfg(target_os = "android")]
pub(crate) const NAME_CLOSE: u16 = 0x7E64;

/// The page's buttons, then its statics in the order they paint.
pub(crate) fn nodes() -> NodeSpec {
    let text = |id: u16, align: ShellAlign| {
        NodeSpec::new(NodeKind::Text)
            .id(id.into())
            .with(Prop::Align, Value::Int(align.0.into()))
    };
    let page = PAGE
        .nodes()
        .child(text(HINT_TEXT, ShellAlign::NONE))
        .children(DETAIL_TEXTS.map(|id| text(id, ShellAlign::NONE)))
        .child(text(PROMPT_TEXT, ShellAlign::H_CENTER).label(PROMPT_KEY));
    #[cfg(target_os = "android")]
    let page = page.children(name_dialog_nodes());
    page
}

/// Android: the name dialog in window pixels, on the message box template's
/// dialog, text body and two buttons, so it sits where the Mods page's boxes
/// do (`App::name_dialog_parts`).
#[cfg(target_os = "android")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NameDialogRects {
    /// The panel: the template's dialog in `PUDLGBGN.SHP`'s shape.
    pub(crate) panel: RectPx,
    /// Where the hint and the name field start, as a box's text does.
    pub(crate) body: RectPx,
    pub(crate) ok: RectPx,
    pub(crate) close: RectPx,
}

#[cfg(target_os = "android")]
impl NameDialogRects {
    /// `aspect`: the panel art's height over its width, when it is loaded;
    /// the panel keeps that shape, centred on the template's dialog.
    pub(crate) fn new(
        dialog: RectPx,
        body: RectPx,
        ok: RectPx,
        close: RectPx,
        aspect: Option<f32>,
    ) -> Self {
        let h = aspect
            .filter(|aspect| aspect.is_finite() && *aspect > 0.0)
            .map_or(dialog.h, |aspect| {
                ((dialog.w as f32 * aspect).round() as i32).max(80)
            });
        let y = dialog.y + (dialog.h - h) / 2;
        Self {
            panel: RectPx::new(dialog.x, y.max(0), dialog.w, h),
            body,
            ok,
            close,
        }
    }

    /// OK's or Close's window.
    pub(crate) fn rect_of(&self, id: u16) -> Option<RectPx> {
        match id {
            NAME_OK => Some(self.ok),
            NAME_CLOSE => Some(self.close),
            _ => None,
        }
    }
}

/// Android: the name dialog's widgets, flat under the page's dialog: the edit,
/// which only takes keys (the overlay paints its field and the hint), OK and
/// Close. `app::shell_mods` shows them and places them.
#[cfg(target_os = "android")]
fn name_dialog_nodes() -> [NodeSpec; 3] {
    let hidden = |spec: NodeSpec| spec.with(Prop::Visible, Value::Bool(false));
    let button =
        |id: u16, key: &str| hidden(NodeSpec::new(NodeKind::Button).id(id.into()).label(key));
    [
        hidden(NodeSpec::new(NodeKind::Edit).id(NAME_EDIT.into())),
        button(NAME_OK, "TXT_OK"),
        button(NAME_CLOSE, "VERA:Close"),
    ]
}

/// The mod actions in the right column, Back on the bottom row.
///
/// Android has no file manager path into the app's storage: Add Mod picks
/// through the system file manager, Open Folder becomes Export Mod through
/// its folder picker, and Rename, Delete and Add Desktop Entry join the
/// column (`docs/android.md`).
#[cfg(target_os = "android")]
pub(crate) const ACTIONS: &[MenuPageButtonSpec] = &[
    MenuPageButtonSpec::new(USE, 122, "VERA:ModUse", "VERA:ModUseHelp"),
    MenuPageButtonSpec::new(CHECK, 149, "VERA:ModCheck", "VERA:ModCheckHelp"),
    MenuPageButtonSpec::new(NEW, 176, "VERA:ModNew", "VERA:ModNewHelp"),
    MenuPageButtonSpec::new(ADD, 203, "VERA:ModAdd", "VERA:ModAddHelp"),
    MenuPageButtonSpec::new(OPEN_FOLDER, 230, "VERA:ModExport", "VERA:ModExportHelp"),
    MenuPageButtonSpec::new(RENAME, 257, "VERA:ModRename", "VERA:ModRenameHelp"),
    MenuPageButtonSpec::new(DELETE, 284, "VERA:ModDelete", "VERA:ModDeleteHelp"),
    MenuPageButtonSpec::new(PIN, 311, "VERA:ModPin", "VERA:ModPinHelp"),
];

#[cfg(not(target_os = "android"))]
pub(crate) const ACTIONS: &[MenuPageButtonSpec] = &[
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
];

pub(crate) const PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DIALOG,
    title_key: TITLE_KEY,
    stacked: ACTIONS,
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

impl ModsLayout {
    /// Control `id`'s window: a button, or one of the page's statics. The
    /// hint runs across the list's rows from the third, eight pixels in from
    /// either side, to the list's end; it shows only while no mod is in the
    /// folder, so the list then has no scrollbar.
    pub(crate) fn rect_of(&self, id: u16) -> Option<RectPx> {
        if let Some(index) = DETAIL_TEXTS.iter().position(|text| *text == id) {
            return Some([self.name, self.state, self.author, self.about, self.terms][index]);
        }
        match id {
            PROMPT_TEXT => Some(self.prompt),
            HINT_TEXT => {
                let metrics = ListMetrics::scaled(self.page.scale);
                let list = ShellListGeometry::new(list_box_rect(self.list), 0, 0, metrics);
                let (content, top) = (list.content, list.row(2).y);
                Some(RectPx::new(
                    content.x + 8,
                    top,
                    content.w - 16,
                    (content.y + content.h - top).max(0),
                ))
            }
            _ => self.page.button_rect(id),
        }
    }
}

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
