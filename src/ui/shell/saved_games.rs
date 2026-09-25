//! Active saved-game children B7/2B4/2B5, owned by original 0x00558DD0.
//! Resource bytes and original 60B7A0 comparisons are preserved in
//! tools/storage_oracle/saved_game_layout.{py,json}. See the pause-save-shells
//! evidence dated 2026-09-12 for callers, text keys and active paint policy.

use super::descriptor::DialogId;
use super::geom::{self, RectPx};
use super::in_game_shell::InGameShellLayout;
use super::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::skirmish_shell::{SavedSeedLayout, SavedSeedMode};

#[derive(Clone, Copy)]
struct Resource {
    list: RectPx,
    prompt: RectPx,
}

fn resource(mode: SavedSeedMode) -> Resource {
    match mode {
        SavedSeedMode::Load => Resource {
            list: RectPx::new(79, 78, 266, 187),
            prompt: RectPx::new(79, 44, 266, 24),
        },
        SavedSeedMode::Save => Resource {
            list: RectPx::new(79, 80, 266, 157),
            prompt: RectPx::new(79, 46, 266, 24),
        },
        SavedSeedMode::Delete => Resource {
            list: RectPx::new(78, 76, 266, 187),
            prompt: RectPx::new(78, 42, 266, 24),
        },
    }
}

const EDIT_DLU: RectPx = RectPx::new(80, 253, 266, 14);
const ACTION_DLU: RectPx = RectPx::new(425, 122, 108, 22);

/// Original saved-game vtable getters 55A050/55A070/55A090. The common
/// SavedSeedMode action/prompt labels also apply to these retail resources.
pub const fn saved_game_title_label(mode: SavedSeedMode) -> (&'static str, &'static str) {
    match mode {
        SavedSeedMode::Load => ("GUI:LoadMissionMenu", "Load Mission"),
        SavedSeedMode::Save => ("GUI:SaveMissionMenu", "Save Mission"),
        SavedSeedMode::Delete => ("GUI:DeleteMissionMenu", "Delete Mission"),
    }
}

fn ordinary_rect(dlu: RectPx, width: i32, height: i32) -> RectPx {
    let raw = geom::dlu_rect(dlu.x, dlu.y, dlu.w, dlu.h);
    // Original60B7A0 clamps final coordinates, not the signed half-delta.
    RectPx::new(
        (raw.x + (width - 800) / 2).max(0),
        (raw.y + (height - 600) / 2).max(0),
        raw.w,
        raw.h,
    )
}

/// Physical active-session layout. Static40C remains visible: 558F8A hides
/// it only for the inactive pre-match browser. Caller supplies loaded SHP sizes.
pub fn saved_game_layout(
    mode: SavedSeedMode,
    width: i32,
    height: i32,
    shell: InGameShellLayout,
    button_size: [i32; 2],
) -> SavedSeedLayout {
    let controls = resource(mode);
    let screen = RectPx::new(0, 0, width, height);
    let action = ACTION_DLU;
    SavedSeedLayout {
        screen,
        dialog: screen,
        // 608CD0 routes694 through active60B1D0; no launcher +7 adjustment.
        title: RectPx::new(width - 165, 2, 162, 16),
        prompt: ordinary_rect(controls.prompt, width, height),
        list: ordinary_rect(controls.list, width, height),
        name_edit: (mode == SavedSeedMode::Save).then(|| ordinary_rect(EDIT_DLU, width, height)),
        action: shell.button_rect(
            geom::dlu_rect(action.x, action.y, action.w, action.h),
            button_size,
        ),
        // 609730 ->60B350: bottom button follows the SIDE3 upper edge.
        back: RectPx::new(
            width - 147,
            shell.side3.y - button_size[1],
            button_size[0],
            button_size[1],
        ),
        // Active60B550 uses parent left+10 and bottom-existingheight-1.
        blank: RectPx::new(10, height - 21, 455, 20),
    }
}

/// Load `0x40F`: the only top button of `0xB7` (`0x006091A5`).
pub const LOAD_BUTTON: u16 = 0x040F;
/// Back `0x686`, the bottom button (`0x00609844`).
pub const BACK_BUTTON: u16 = 0x0686;
/// Status help of the list `0x525` (`0x00604261..0x0060429A`).
pub const LOAD_LIST_HELP_KEY: &str = "STT:LoadList";

/// `0xB7` opened from Single Player (`Main__PrepareSession` state 9,
/// `0x0052E0BE`): a family page with the right panel, heading and status
/// line. Proc `0x00558A30` writes `0x40F` for Load and 2 for Back
/// (`0x00558AB0`).
pub const LOAD_SAVED_GAME_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogId(0x00B7),
    title_key: "GUI:LoadMissionMenu",
    stacked: &[MenuPageButtonSpec {
        id: LOAD_BUTTON,
        dlu_top: ACTION_DLU.y,
        csf_key: "GUI:Load",
        tooltip_key: "STT:LoadButtonLoad",
        result: Some(LOAD_BUTTON as i32),
    }],
    back: MenuPageButtonSpec {
        id: BACK_BUTTON,
        dlu_top: 253,
        csf_key: "GUI:Back",
        tooltip_key: "STT:LoadButtonBack",
        result: Some(2),
    },
};

/// The main-menu `0xB7`: the page and the browser geometry its list, Load
/// and Back share.
#[derive(Debug, Clone)]
pub struct MainMenuSavedGameLayout {
    pub page: MenuPageLayout,
    /// The list window `0x525`: the template rectangle, which `0x0060B7A0`
    /// leaves in place outside a suspended game (`0x0060B7BC`).
    pub list_window: RectPx,
    pub browser: SavedSeedLayout,
}

/// Layout of the main-menu `0xB7`. The browser list is the paint surface,
/// one pixel wider and taller than the window, like every family list.
/// The prompt `0x40C` is hidden here (`0x00558F7C..0x00558F99`).
pub fn main_menu_saved_game_layout(width: u32, height: u32) -> MainMenuSavedGameLayout {
    let page = menu_page::compute_layout(&LOAD_SAVED_GAME_PAGE, width, height);
    let controls = resource(SavedSeedMode::Load);
    let list_window = geom::dlu_rect(
        controls.list.x,
        controls.list.y,
        controls.list.w,
        controls.list.h,
    );
    let button = |id| page.button_rect(id).expect("0xB7 page button");
    let browser = SavedSeedLayout {
        screen: page.screen,
        dialog: page.screen,
        title: page.title,
        prompt: geom::dlu_rect(
            controls.prompt.x,
            controls.prompt.y,
            controls.prompt.w,
            controls.prompt.h,
        ),
        list: RectPx::new(
            list_window.x,
            list_window.y,
            list_window.w + 1,
            list_window.h + 1,
        ),
        name_edit: None,
        action: button(LOAD_BUTTON),
        back: button(BACK_BUTTON),
        blank: page.status_help,
    };
    MainMenuSavedGameLayout {
        page,
        list_window,
        browser,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(values: &serde_json::Value) -> RectPx {
        let n = |i: usize| values[i].as_i64().unwrap() as i32;
        RectPx::new(n(0), n(1), n(2), n(3))
    }

    fn mode(id: u64) -> SavedSeedMode {
        match id {
            0xb7 => SavedSeedMode::Load,
            0x2b4 => SavedSeedMode::Save,
            0x2b5 => SavedSeedMode::Delete,
            _ => panic!("unexpected native resource"),
        }
    }

    #[test]
    fn main_menu_load_page_takes_the_family_rects() {
        let layout = main_menu_saved_game_layout(800, 600);
        // 0x0060B7A0 leaves the template list in place outside a suspended
        // game; the browser list is its paint surface.
        assert_eq!(layout.list_window, RectPx::new(119, 127, 399, 304));
        assert_eq!(layout.browser.list, RectPx::new(119, 127, 400, 305));
        assert_eq!(layout.browser.title, RectPx::new(635, 9, 163, 18));
        assert_eq!(layout.browser.blank, RectPx::new(10, 578, 456, 21));
        assert_eq!(layout.browser.action, RectPx::new(644, 199, 156, 42));
        assert_eq!(layout.browser.back, RectPx::new(644, 535, 156, 42));
        assert_eq!(layout.browser.name_edit, None);
    }

    #[test]
    fn ordinary_controls_match_original_resource_bytes_and_native_placement() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/storage_oracle/saved_game_layout.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let mode = mode(case["dialog_id"].as_u64().unwrap());
            let control = case["control_id"].as_u64().unwrap();
            let source = match control {
                0x40c => resource(mode).prompt,
                0x525 | 0x527 | 0x528 => resource(mode).list,
                0x526 => EDIT_DLU,
                _ => panic!("unexpected ordinary control"),
            };
            assert_eq!(source, rect(&case["dlu_rect"]));
            assert_eq!(
                ordinary_rect(
                    source,
                    case["width"].as_i64().unwrap() as i32,
                    case["height"].as_i64().unwrap() as i32,
                ),
                rect(&case["rect"]),
                "dialog {mode:?}, control {control:x}"
            );
        }
    }
}
