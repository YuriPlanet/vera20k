//! Pixel-parity Skirmish shell model and layout.
//!
//! This module owns render-agnostic dialog 0x102 geometry, state, and hit
//! testing. Rendering code consumes the computed rects from the app/render
//! layers; this module does not depend on assets or wgpu.

mod layout;
pub mod choose_map_page;
pub mod page;
pub mod random_map_page;
mod state;
mod statics;
pub(crate) use statics::SkirmishStatics;

pub use layout::{
    CHOOSE_MAP_MODAL_H, CHOOSE_MAP_MODAL_W, CHOOSE_MAP_TITLE_KEY, COMBO_ARROW_RESERVE_W,
    COMBO_DROPDOWN_ROW_H, COMBO_DROPDOWN_SCROLLBAR_BUTTON_H, COMBO_DROPDOWN_SCROLLBAR_MIN_THUMB_H,
    COMBO_DROPDOWN_SCROLLBAR_W, COMBO_FACE_H, COMBO_TEXT_LEFT_INSET, ChooseMapModalButton,
    AdvancedField, ChooseMapModalLayout, ColorComboId, DropdownMetrics, RANDOM_MAP_TITLE_KEY, RIGHT_PANEL_WIDTH,
    RandomMapSetupControl, RandomMapSetupLayout, RectPx, SavedSeedLayout,
    SavedSeedMode, ShellControlId, SkirmishCheckboxId, SkirmishCheckboxRect,
    SkirmishColumnLabelRects, SkirmishRightPanelTextRects, SkirmishRowRects, SkirmishShellLayout,
    SkirmishTrackbarId, SkirmishTrackbarLabelRects, SkirmishTrackbarRects, checkbox_icon_rect,
    checkbox_text_rect, combo_arrow_hit_rect, combo_arrow_rect,
    combo_dropdown_row_rect, combo_face_rect, combo_swatch_rect, combo_text_rect,
    SKIRMISH_LONG_ROW_COUNT, SKIRMISH_ROW_COUNT, compute_choose_map_modal_layout, compute_fixed_800_layout,
    compute_layout, compute_layout_for_rows, compute_layout_on,
    compute_random_map_setup_layout, compute_responsive_choose_map_modal_layout,
    compute_responsive_layout, compute_responsive_layout_for_rows,
    compute_responsive_random_map_setup_layout,
    compute_responsive_saved_seed_layout, compute_responsive_setups_layout, compute_saved_seed_layout,
    compute_setups_layout, player_name_edit_client_rect,
    player_name_edit_text_rect, random_map_setup_dropdown_rect,
};
pub(crate) use state::CountryMenuCatalog;
pub(crate) use state::status_help_key_for_hover_with_catalog;
pub use state::{
    AcceptOutcome, ChooseMapContext, ChooseMapHoverTarget, ChooseMapModalState,
    ChooseMapPrompt, ChooseMapPromptAnswer, ChooseMapPromptButton, ChooseMapPromptKind, ChooseMapSelection,
    FAVORITES_ROW, ONLINE_MAPS_ROW, SEARCH_MAX_CHARS, SkirmishModeSelection, favorite_key,
    DropdownScrollbarPart, MainRngRanged,
    OpenComboDropdown, OwnerDrawButton, PLAYER_NAME_CARET_MARGIN_PX, PLAYER_NAME_DEFAULT,
    PLAYER_NAME_EDIT_LIMIT_BYTES, PLAYERS_RANGE, ADVANCED_RANGE, PlayerNameEditState,
    RandomMapPreset, RandomMapSetupModalState, CUSTOM_PRESET_SLOTS,
    EXTRAS_ROWS, ExtrasAction, ExtrasOutcome, ExtrasRow, ExtrasValue, extras_page_count,
    SAVED_SEED_DESCRIPTION_MAX_UNITS, SETUP_COMBO_ROWS, SavedSeedBrowserRow, SavedSeedBrowserState,
    SavedSeedDescriptionEdit, SavedSeedOutcome, SavedSeedPrompt, SavedSeedPromptPurpose,
    SetupCombo, SetupComboItem, SkirmishAiRowType, SkirmishComboId, SkirmishComboItem,
    SkirmishCountryChoice, SkirmishHoverTarget, SkirmishSavedSetup, SkirmishSetupComputer,
    starts_by_team,
    SkirmishSetupSeat, SkirmishShellAction, SkirmishShellDialog, SkirmishShellOpponent,
    SkirmishShellState, SkirmishShellUiSound, SkirmishTrackbarBounds,
    SkirmishValidationModalState, accept_selected_map,
    ai_rows_beyond_limit_occupied, apply_action, available_color_choices, available_start_choices,
    clear_status_help_text, combo_dropdown_max_visible_rows, combo_enabled, combo_items, combo_rect, country_choices,
    dismiss_validation_modal, drain_pending_ui_sounds, handle_option_mouse_wheel, move_opponent_scrollbar, press_opponent_scrollbar,
    initialize_rows_for_selected_map, launch_session, opponent_scroll_deadline, opponent_scroll_geometry, opponent_scroll_held,
    pack_launch_session_without_start_validation, player_row_visible, poll_opponent_scroll, selected_player_rows,
    repair_teams_for_selected_mode,
    selected_combo_item, selected_combo_item_index, set_status_help_text, setup_combo_items,
    status_help_key_for_choose_map_hover, status_help_key_for_hover,
    status_help_key_for_random_map_setup, team_choices,
    };

pub mod game_options;
pub(crate) use state::saved_seed_browser;
