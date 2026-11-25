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
/// Android: remove the selected mod's folder (the app owns it).
pub(crate) const DELETE: u16 = 0x7E37;
/// Android: rename the selected mod's folder.
pub(crate) const RENAME: u16 = 0x7E39;
/// Android: add a home-screen shortcut for the selected mod.
pub(crate) const PIN: u16 = 0x7E38;
pub(crate) const ADD: u16 = 0x7E36;

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

/// Android: New Mod's name dialog, a page dialog rather than a message box so
/// the edit keeps its text and focus across frames. Hidden until New Mod opens
/// it (`app::shell_mods`).
#[cfg(target_os = "android")]
pub(crate) const NAME_PROMPT: u16 = 0x7E61;
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

/// Android: New Mod's dialog parts in window pixels, on the box template's
/// own dialog rect so it sits where Add Mod's and the Mods page's boxes do.
/// The panel keeps `PUDLGBGN.SHP`'s shape: its height follows the art's
/// aspect, which the renderer sets from the loaded entry.
#[cfg(target_os = "android")]
pub(crate) fn name_dialog_rects(
    scale: crate::ui::shell::geom::ShellScale,
) -> (
    crate::ui::shell::geom::RectPx,
    crate::ui::shell::geom::RectPx,
    crate::ui::shell::geom::RectPx,
    crate::ui::shell::geom::RectPx,
    crate::ui::shell::geom::RectPx,
) {
    use crate::ui::shell::geom::RectPx;
    let (dx, dy, dw, dh) = name_box();
    let (ox, oy, ow, oh) = name_button(0);
    let (cx, cy, cw, ch) = name_button(1);
    let window = scale.window();
    let windowed = |x: f32, y: f32, w: f32, h: f32| {
        RectPx::new(
            (x * window.w as f32).round() as i32,
            (y * window.h as f32).round() as i32,
            (w * window.w as f32).round() as i32,
            (h * window.h as f32).round() as i32,
        )
    };
    let dialog = windowed(dx, dy, dw, dh);
    let aspect = name_aspect();
    let h = ((dialog.w as f32 * aspect).round() as i32).max(80);
    let y = dialog.y + (dialog.h - h) / 2;
    let panel = RectPx::new(dialog.x, y.max(0), dialog.w, h);
    let inset = (panel.w as f32 * 0.03).round() as i32;
    let at = |x: i32, y: i32, w: i32, h: i32| RectPx::new(x, y, w, h);
    (
        panel,
        at(
            panel.x + inset,
            panel.y + inset,
            panel.w - 2 * inset,
            (panel.h as f32 * 0.12) as i32,
        ),
        at(
            panel.x + inset,
            panel.y + inset + (panel.h as f32 * 0.16) as i32,
            panel.w - 2 * inset,
            (panel.h as f32 * 0.16) as i32,
        ),
        windowed(ox, oy, ow, oh),
        windowed(cx, cy, cw, ch),
    )
}

/// Android: the message box template's dialog and its first two buttons, as
/// fractions of the window, written by the renderer once a frame.
#[cfg(target_os = "android")]
pub(crate) fn set_name_box(
    dialog: crate::ui::shell::geom::RectPx,
    body: crate::ui::shell::geom::RectPx,
    ok: crate::ui::shell::geom::RectPx,
    close: crate::ui::shell::geom::RectPx,
    window: crate::ui::shell::geom::RectPx,
) {
    let write = |base: usize, rect: crate::ui::shell::geom::RectPx| {
        let values = [
            rect.x as f32 / window.w.max(1) as f32,
            rect.y as f32 / window.h.max(1) as f32,
            rect.w as f32 / window.w.max(1) as f32,
            rect.h as f32 / window.h.max(1) as f32,
        ];
        for (index, value) in values.iter().enumerate() {
            NAME_BOX[base + index].store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
        }
    };
    write(0, dialog);
    write(4, ok);
    write(8, close);
    write(12, body);
}

#[cfg(target_os = "android")]
fn read_box(base: usize) -> (f32, f32, f32, f32) {
    let read = |index: usize| {
        f32::from_bits(NAME_BOX[base + index].load(std::sync::atomic::Ordering::Relaxed))
    };
    (read(0), read(1), read(2), read(3))
}

#[cfg(target_os = "android")]
fn name_box() -> (f32, f32, f32, f32) {
    read_box(0)
}

/// Android: the box template's body rect, where the message boxes' text starts.
#[cfg(target_os = "android")]
pub(crate) fn name_body_rect(
    scale: crate::ui::shell::geom::ShellScale,
) -> crate::ui::shell::geom::RectPx {
    let (x, y, w, h) = read_box(12);
    let window = scale.window();
    crate::ui::shell::geom::RectPx::new(
        (x * window.w as f32).round() as i32,
        (y * window.h as f32).round() as i32,
        (w * window.w as f32).round() as i32,
        (h * window.h as f32).round() as i32,
    )
}

#[cfg(target_os = "android")]
fn name_button(index: usize) -> (f32, f32, f32, f32) {
    read_box(if index == 0 { 4 } else { 8 })
}

/// Android: the art's height over its width, as the renderer last read it.
#[cfg(target_os = "android")]
pub(crate) fn set_name_aspect(aspect: f32) {
    if aspect.is_finite() && aspect > 0.0 {
        NAME_ASPECT.store(aspect.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(target_os = "android")]
fn name_aspect() -> f32 {
    f32::from_bits(NAME_ASPECT.load(std::sync::atomic::Ordering::Relaxed))
}

#[cfg(target_os = "android")]
static NAME_ASPECT: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0.75f32.to_bits());

#[cfg(target_os = "android")]
static NAME_BOX: [std::sync::atomic::AtomicU32; 16] = [
    std::sync::atomic::AtomicU32::new(0.2f32.to_bits()),
    std::sync::atomic::AtomicU32::new(0.3f32.to_bits()),
    std::sync::atomic::AtomicU32::new(0.6f32.to_bits()),
    std::sync::atomic::AtomicU32::new(0.4f32.to_bits()),
    std::sync::atomic::AtomicU32::new(0.22f32.to_bits()),
    std::sync::atomic::AtomicU32::new(0.62f32.to_bits()),
    std::sync::atomic::AtomicU32::new(0.16f32.to_bits()),
    std::sync::atomic::AtomicU32::new(0.06f32.to_bits()),
    std::sync::atomic::AtomicU32::new(0.62f32.to_bits()),
    std::sync::atomic::AtomicU32::new(0.62f32.to_bits()),
    std::sync::atomic::AtomicU32::new(0.16f32.to_bits()),
    std::sync::atomic::AtomicU32::new(0.06f32.to_bits()),
    std::sync::atomic::AtomicU32::new(0.25f32.to_bits()),
    std::sync::atomic::AtomicU32::new(0.38f32.to_bits()),
    std::sync::atomic::AtomicU32::new(0.50f32.to_bits()),
    std::sync::atomic::AtomicU32::new(0.20f32.to_bits()),
];

/// New Mod's name dialog widgets, flat under the page's dialog so the page
/// painter draws them: the hint, the edit, OK and Close. `app::shell_mods`
/// shows them and places them.
#[cfg(target_os = "android")]
fn name_dialog_nodes() -> [NodeSpec; 4] {
    let hidden = |spec: NodeSpec| spec.with(Prop::Visible, Value::Bool(false));
    let button =
        |id: u16, key: &str| hidden(NodeSpec::new(NodeKind::Button).id(id.into()).label(key));
    [
        hidden(
            NodeSpec::new(NodeKind::Text)
                .id(NAME_PROMPT.into())
                .label("VERA:ModNewNameHint"),
        ),
        hidden(NodeSpec::new(NodeKind::Edit).id(NAME_EDIT.into())),
        button(NAME_OK, "TXT_OK"),
        button(NAME_CLOSE, "VERA:Close"),
    ]
}

/// The mod actions in the right column, Back on the bottom row.
///
/// Android has no file manager path into the app's storage: `ADD`'s desktop
/// drag-and-drop is replaced by Import Mod inside New Mod, Open Folder becomes
/// Export Mod through the system picker, and Delete and Add Desktop Entry join
/// the column (`docs/android.md`).
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
        #[cfg(target_os = "android")]
        {
            let (_, prompt, _, ok, close) = name_dialog_rects(self.page.scale);
            match id {
                NAME_PROMPT => return Some(prompt),
                NAME_OK => return Some(ok),
                NAME_CLOSE => return Some(close),
                _ => {}
            }
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
