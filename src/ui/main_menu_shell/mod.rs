//! Initial main-menu shell dialog 0xE2 layout and input state.

mod layout;
mod state;
mod typed_codes;

pub use layout::{
    MainMenuButtonRect, MainMenuMovieBase, MainMenuShellLayout, RA2TS_BASE_MOVIE,
    RIGHT_PANEL_TILE_H, RIGHT_PANEL_WIDTH, RectPx, SHELL_BASE_H, SHELL_BASE_W, compute_layout,
    compute_responsive_layout, movie_base_for_responsive_width, movie_base_for_screen_width,
};
pub use state::{
    MAIN_MENU_DIALOG, MainMenuControlId, MainMenuShellAction, MainMenuShellState,
    action_for_control, csf_key_for_control, hovered_control, pressed_control,
    return_code_for_action, tooltip_csf_key_for_control,
};
pub use typed_codes::{MainMenuTypedCode, MainMenuTypedCodes, VideoCodeFlags};
