//! Pixel-parity Skirmish shell model and layout.
//!
//! This module owns render-agnostic dialog 0x102 geometry, state, and hit
//! testing. Rendering code consumes the computed rects from the app/render
//! layers; this module does not depend on assets or wgpu.

pub mod choose_map_page;
mod layout;
pub mod page;
pub mod random_map_page;
mod state;
mod statics;
pub(crate) use statics::SkirmishStatics;

pub use layout::{
    AdvancedField, CHOOSE_MAP_AUTHOR, CHOOSE_MAP_TITLE_KEY,
    COMBO_ARROW_RESERVE_W, COMBO_DROPDOWN_ROW_H, COMBO_DROPDOWN_SCROLLBAR_BUTTON_H,
    COMBO_DROPDOWN_SCROLLBAR_MIN_THUMB_H, COMBO_DROPDOWN_SCROLLBAR_W, COMBO_FACE_H,
    COMBO_TEXT_LEFT_INSET, ChooseMapModalButton, ChooseMapModalLayout, DropdownMetrics,
    RANDOM_MAP_TITLE_KEY, RIGHT_PANEL_WIDTH, RandomMapSetupControl,
    RandomMapSetupLayout, RectPx, SKIRMISH_LONG_ROW_COUNT, SKIRMISH_ROW_COUNT, SavedSeedLayout,
    SavedSeedMode, SkirmishCheckboxId,
    SkirmishRowRects, SkirmishShellLayout, GAME_TYPE_STATIC, MAP_LABEL_STATIC, TITLE_STATIC,
    SkirmishTrackbarId, checkbox_icon_rect,
    checkbox_text_rect, combo_arrow_hit_rect, combo_arrow_rect, combo_dropdown_row_rect,
    combo_face_rect, combo_swatch_rect, combo_text_rect, compute_choose_map_modal_layout,
    compute_layout, compute_layout_for_rows, compute_layout_on,
    compute_random_map_setup_layout, compute_saved_seed_layout, compute_setups_layout,
    random_map_setup_dropdown_rect, FRAMED_EDIT_TEXT_INSET,
};
pub(crate) use state::CountryMenuCatalog;
pub(crate) use state::status_help_key_for_hover_with_catalog;
pub use state::{
    ADVANCED_RANGE, AcceptOutcome, CUSTOM_PRESET_SLOTS, ChooseMapContext, ChooseMapHoverTarget,
    ChooseMapModalState, ChooseMapPrompt, ChooseMapPromptAnswer, ChooseMapPromptButton,
    ChooseMapPromptKind, ChooseMapSelection, DropdownScrollbarPart, EXTRAS_ROWS, ExtrasAction,
    ExtrasOutcome, ExtrasRow, ExtrasValue, FAVORITES_ROW, MainRngRanged, ONLINE_MAPS_ROW, RANDOM_MAPS_ROW,
    OpenComboDropdown, OwnerDrawButton, PLAYER_NAME_DEFAULT, PLAYER_NAME_EDIT_LIMIT_BYTES,
    PLAYERS_RANGE, RandomMapPreset,
    RandomMapSetupModalState, SAVED_SEED_DESCRIPTION_MAX_UNITS, SEARCH_MAX_CHARS, SETUP_COMBO_ROWS,
    SavedSeedBrowserRow, SavedSeedBrowserState, SavedSeedOutcome,
    SavedSeedPrompt, SavedSeedPromptPurpose, SetupCombo, SetupComboItem, SkirmishAiRowType,
    SkirmishComboId, SkirmishComboItem, SkirmishCountryChoice, SkirmishHoverTarget,
    SkirmishModeSelection, SkirmishSavedSetup, SkirmishSetupComputer, SkirmishSetupSeat,
    SkirmishShellAction, SkirmishShellDialog, SkirmishShellOpponent, SkirmishShellState,
    SkirmishTrackbarBounds, accept_selected_map, ai_rows_beyond_limit_occupied,
    available_color_choices, available_start_choices, combo_dropdown_max_visible_rows,
    combo_enabled, combo_items, combo_rect, country_choices, extras_page_count, favorite_key,
    initialize_rows_for_selected_map, launch_session, opponent_scroll_geometry,
    opponent_scroll_rows, pack_launch_session_without_start_validation, player_name_from_profile,
    player_row_visible, repair_teams_for_selected_mode,
    selected_combo_item, selected_combo_item_index, selected_player_rows, setup_combo_items,
    starts_by_team, MenuPick, apply_seat_action, apply_team_colors, apply_team_preset,
    apply_team_starts, balance_teams_by_strength, team_start_seats, skirmish_session_options,
    status_help_key_for_choose_map_hover,
    status_help_key_for_hover, status_help_key_for_random_map_setup, team_choices,
};

pub mod game_options;
pub(crate) use state::saved_seed_browser;
