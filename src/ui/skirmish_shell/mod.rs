//! Pixel-parity Skirmish shell model and layout.
//!
//! This module owns render-agnostic dialog 0x102 geometry, state, and hit
//! testing. Rendering code consumes the computed rects from the app/render
//! layers; this module does not depend on assets or wgpu.

mod layout;
mod scroll;
pub(crate) use scroll::ScrollModel;
mod state;
mod statics;
pub(crate) use statics::SkirmishStatics;

pub use layout::{
    CHOOSE_MAP_MODAL_H, CHOOSE_MAP_MODAL_W, CHOOSE_MAP_TITLE_KEY, COMBO_ARROW_RESERVE_W,
    COMBO_DROPDOWN_ROW_H, COMBO_DROPDOWN_SCROLLBAR_BUTTON_H, COMBO_DROPDOWN_SCROLLBAR_MIN_THUMB_H,
    COMBO_DROPDOWN_SCROLLBAR_W, COMBO_FACE_H, COMBO_TEXT_LEFT_INSET, ChooseMapModalButton,
    ChooseMapModalLayout, ColorComboId, RANDOM_MAP_TITLE_KEY, RIGHT_PANEL_WIDTH,
    RandomMapSetupControl, RandomMapSetupLayout, RectPx, SavedSeedControl, SavedSeedLayout,
    SavedSeedMode, ShellControlId, SkirmishCheckboxId, SkirmishCheckboxRect,
    SkirmishColumnLabelRects, SkirmishRightPanelTextRects, SkirmishRowRects, SkirmishShellLayout,
    SkirmishTrackbarId, SkirmishTrackbarLabelRects, SkirmishTrackbarRects, checkbox_icon_rect,
    checkbox_text_rect, choose_map_modal_button_at, combo_arrow_rect, combo_face_rect,
    combo_swatch_rect, combo_text_rect, compute_choose_map_modal_layout, compute_fixed_800_layout,
    compute_layout, compute_random_map_setup_layout, compute_saved_seed_layout,
    player_name_edit_client_rect, player_name_edit_text_rect, random_map_setup_control_at,
    random_map_setup_dropdown_rect, random_map_setup_dropdown_row_at, saved_seed_control_at,
    trackbar_active_width, trackbar_pixel_offset, trackbar_plaque_rect, trackbar_thumb_rect,
    trackbar_value_text_rect,
};
pub use state::{
    AcceptOutcome, ChooseMapHoverTarget, ChooseMapListPress, ChooseMapModalState,
    ChooseMapSelection, DropdownScrollDragState, DropdownScrollbarPart,
    DropdownScrollbarPressState, EjectPrompt, EjectPromptButton, OpenComboDropdown,
    OwnerDrawButton, PLAYER_NAME_CARET_MARGIN_PX, PLAYER_NAME_DEFAULT,
    PLAYER_NAME_EDIT_LIMIT_BYTES, PlayerNameEditState, RandomMapSetupModalState,
    SAVED_SEED_DESCRIPTION_MAX_UNITS, SETUP_COMBO_ROWS, SavedSeedBrowserRow, SavedSeedBrowserState,
    SavedSeedOutcome, SavedSeedPrompt, SavedSeedPromptPurpose, SetupCombo, SetupComboItem,
    SkirmishAiRowType, SkirmishComboId, SkirmishComboItem, SkirmishCountryChoice,
    SkirmishHoverTarget, SkirmishShellAction, SkirmishShellDialog, SkirmishShellOpponent,
    SkirmishShellState, SkirmishShellUiSound, SkirmishTrackbarBounds, SkirmishValidationModalState,
    TrackbarDragState, accept_selected_map, action_for_owner_draw_button,
    ai_rows_beyond_limit_occupied, apply_action,
    blur_player_name_edit, clear_status_help_text, combo_dropdown_content_rect,
    combo_dropdown_needs_scrollbar, combo_dropdown_open, combo_dropdown_rect,
    combo_dropdown_scroll_thumb_rect, combo_dropdown_scrollbar_rect,
    combo_dropdown_visible_row_count, combo_enabled, combo_items, combo_rect,
    dismiss_validation_modal, drain_pending_ui_sounds, focus_player_name_edit,
    game_speed_from_visual_position, game_speed_visual_position, handle_option_mouse_down,
    handle_option_mouse_move, handle_option_mouse_up, handle_option_mouse_wheel,
    handle_player_name_backspace, handle_player_name_delete, handle_player_name_end,
    handle_player_name_home, handle_player_name_left, handle_player_name_right,
    handle_player_name_tab, hit_test, hit_test_owner_draw_button, hovered_choose_map_modal_control,
    hovered_shell_control, initialize_rows_for_selected_map, insert_player_name_text,
    launch_session, launch_settings, pack_launch_session_without_start_validation,
    player_name_caret_prefix, player_name_edit_rect_hit, player_row_visible,
    repair_teams_for_selected_mode, selected_combo_item, selected_combo_item_index,
    set_status_help_text, setup_combo_items, status_help_key_for_choose_map_hover,
    status_help_key_for_hover, status_help_key_for_random_map_setup, trackbar_mouse_allowed_y,
    trackbar_mouse_value, trackbar_thumb_hit, trackbar_visual_value,
    update_player_name_scroll_for_caret,
};

pub mod seed_list;
