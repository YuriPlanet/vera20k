# Module dependency map

<!-- module-map:provenance:begin -->
Generated snapshot: `96135ec43fb28d17b7aeab61507fe97c29edc151` (2026-09-21), cargo-modules 0.26.0.

Scope: `vera20k` library, default features, `x86_64-pc-windows-msvc`, no depth limit.
Test-only, binary-specific and inactive conditional modules are excluded.
External crates and the sysroot are excluded from the dependency graph.
Contains **839 modules plus the crate root**, and **5312 distinct
cross-module dependency edges**. Check this source commit against your checkout.
<!-- module-map:provenance:end -->

`A -> B; C` lists A's dependencies. Paths omit `vera20k::`; `vera20k` is the root.
Rows cover individual modules, not their children; `-` means no emitted dependency.
These are static imports/type relationships, not runtime calls. Verify against current source.
An absent edge does not prove independence; follow source references, including parameter types.
Search relevant rows only; follow destinations to trace chains, avoiding revisited modules.

```powershell
# Dependencies
rg -n '^sim::movement -> ' docs/module-map.md
# Dependents (exact destination)
rg -n -- ' -> (.*; )?sim::movement(;|$)' docs/module-map.md
```

Refresh in the relevant checkout with committed source/build inputs; follow [ENGINE.md](../ENGINE.md).

```powershell
python tools/module_map.py
python tools/module_map.py --check
```

## Generated dependency index

<!-- module-map:dependencies:begin -->
```text
app -> app::diagnostics::shell_capture; app::frontend::launch; app::frontend::startup_options; app::loading::transitions; app::match_runtime::scenario_exit; app::match_runtime::sim_tick; app::presentation::render; app::shell_random_map; app::state; app::state::platform; assets::asset_manager; audio::music; audio::sfx; map::basic; map::houses; map::map_file; map::resolved_terrain; map::rmg; map::rmg::description; map::rmg::options; render::batch; render::bit_font; render::egui_integration; render::gpu; rules::overlay_types; rules::ruleset; sidebar; sidebar::layout_spec; sim::selection; skirmish_launch; ui::game_screen; ui::main_menu; ui::main_menu_dialogs::options; ui::main_menu_shell::layout; ui::main_menu_shell::state; ui::pause_menu; ui::score_shell; ui::shell::controller; ui::shell::descriptor; ui::shell::saved_file_input; ui::single_player_shell::layout; ui::single_player_shell::state; ui::skirmish_shell::layout; ui::skirmish_shell::state; ui::skirmish_shell::state::choose_map; ui::skirmish_shell::state::random_map_setup; ui::skirmish_shell::state::saved_seed_browser; util::config; util::native_file_name
app::audio_runtime -> assets::asset_manager; audio::music; audio::sfx; audio::theme; rules::sound_ini
app::diagnostics -> -
app::diagnostics::debug_overlays -> app::presentation::instances::helpers; app::state; map::terrain; render::batch; rules::locomotor_type
app::diagnostics::debug_panel -> app::state; map::entities; sim::debug_event_log
app::diagnostics::dev_overlay -> app::diagnostics::debug_panel
app::diagnostics::shell_capture -> app::diagnostics::shell_capture::skirmish; app::frontend::main_menu_shell_render; app::frontend::shell_transition; app::state; render::frame_readback; ui::game_screen; ui::main_menu_shell::layout; ui::shell::slide; ui::shell::static_reveal
app::diagnostics::shell_capture::skirmish -> app; app::diagnostics::shell_capture; app::frontend::main_menu_shell_render; app::frontend::shell_transition; app::state; render::frame_readback; ui::game_screen; ui::main_menu_shell::layout; ui::shell::slide; ui::shell::static_reveal
app::diagnostics::state -> app::diagnostics::dev_overlay
app::diagnostics::tactical_capture -> -
app::diagnostics::tactical_capture::evidence -> app::diagnostics::tactical_capture::integrity; app::presentation::render; render::egui_integration; render::gpu; render::sidebar_chrome; sidebar; sidebar::layout_spec
app::diagnostics::tactical_capture::integrity -> util::sha256
app::diagnostics::tactical_capture::manifest -> app::diagnostics::tactical_capture::integrity; app::diagnostics::tactical_capture::profile
app::diagnostics::tactical_capture::placement -> map::entities; rules::overlay_types; rules::ruleset; sim::pathfinding::core; sim::production; sim::world
app::diagnostics::tactical_capture::profile -> app::diagnostics::tactical_capture::integrity; skirmish_launch
app::diagnostics::tactical_capture::script -> app::diagnostics::tactical_capture::placement
app::diagnostics::tactical_capture::session -> app::diagnostics::tactical_capture::evidence; app::diagnostics::tactical_capture::integrity; app::diagnostics::tactical_capture::manifest; app::diagnostics::tactical_capture::placement; app::diagnostics::tactical_capture::profile; app::diagnostics::tactical_capture::script; app::frontend::launch; app::presentation::render; app::state; match_bootstrap; render::cursor_atlas; render::radar_animation; sidebar::layout_spec; sim::command; sim::house_state; sim::production; sim::world; ui::game_screen
app::frame -> app; app::frontend::startup_splash; app::loading::transitions; app::match_runtime::sim_tick; app::presentation::render; app::state; ui::game_screen; ui::main_menu
app::frontend -> -
app::frontend::launch -> app::diagnostics::shell_capture; app::diagnostics::tactical_capture::integrity; app::diagnostics::tactical_capture::profile; app::frontend::startup_options; skirmish_launch
app::frontend::list_maps -> assets::asset_manager; assets::csf_file; assets::mix_archive; map::briefing; map::map_file; map::preview; map::scenario_menu; map::skirmish_scenarios; rules::ini_parser; util::config
app::frontend::main_menu_shell_render -> app::state; render::batch; render::main_menu_shell_chrome; render::shell_paint; render::shell_text; render::shell_text_reveal; render::shell_transition_pass; ui::main_menu_shell::layout; ui::main_menu_shell::state; ui::shell::geom; ui::shell::slide; ui::shell::static_reveal
app::frontend::quit_cascade -> -
app::frontend::score_shell_render -> app::state; render::batch; render::main_menu_shell_chrome; render::shell_paint; render::shell_text; render::shell_transition_pass; ui::score_shell; ui::shell::geom
app::frontend::shell_transition -> app::state; ui::shell::descriptor; ui::shell::slide; ui::shell::static_reveal
app::frontend::single_player_shell_render -> app::state; render::batch; render::shell_paint; render::shell_text; render::shell_transition_pass; ui::shell::geom; ui::shell::slide; ui::single_player_shell::layout; ui::single_player_shell::state
app::frontend::skirmish -> assets::asset_manager; assets::pal_file; map::houses; map::map_file; map::overlay; map::waypoints; render::batch; render::bridge_atlas; render::bridge_railing_atlas; render::gpu; render::overlay_assets; render::overlay_atlas; rules::art_data; rules::color_scheme; rules::crate_rules; rules::house_colors; rules::ini_parser; rules::overlay_types; rules::ruleset; rules::tiberium_type; sim::entity_store; sim::house_state; sim::scenario_bootstrap; sim::world; skirmish_launch; ui::main_menu
app::frontend::skirmish_session -> assets::asset_manager; map::construction_trace; map::rmg::options; map::scenario_menu; rules::ini_parser; sim::rng; skirmish_cooperative; skirmish_launch; skirmish_modes; skirmish_persistence; ui::main_menu; ui::skirmish_shell::layout; ui::skirmish_shell::state; ui::skirmish_shell::state::player_name
app::frontend::skirmish_shell_render -> app::frontend::skirmish_shell_render::abort; app::frontend::skirmish_shell_render::chrome; app::frontend::skirmish_shell_render::controls; app::frontend::skirmish_shell_render::draw_order; app::frontend::skirmish_shell_render::in_game_shell; app::frontend::skirmish_shell_render::keyboard; app::frontend::skirmish_shell_render::launcher_options; app::frontend::skirmish_shell_render::modals; app::frontend::skirmish_shell_render::pause_menu; app::frontend::skirmish_shell_render::preview; app::frontend::skirmish_shell_render::saved_games; app::frontend::skirmish_shell_render::sound; app::frontend::skirmish_shell_render::text; app::state; map::scenario_menu; render::batch; render::bit_font; render::shell_transition_pass; render::skirmish_shell_chrome; rules::color_scheme; skirmish_modes; ui::main_menu; ui::shell::geom; ui::shell::slide; ui::skirmish_shell::layout; ui::skirmish_shell::state; ui::skirmish_shell::state::player_name
app::frontend::skirmish_shell_render::abort -> app::frontend::skirmish_shell_render::in_game_shell; app::frontend::skirmish_shell_render::pause_menu; app::frontend::skirmish_shell_render::text; app::state; render::shell_text; ui::shell::abort; ui::shell::geom; ui::shell::pause_menu
app::frontend::skirmish_shell_render::chrome -> app::frontend::skirmish_shell_render::draw_order; render::batch; render::shell_paint; render::skirmish_shell_chrome; ui::shell::geom; ui::skirmish_shell::layout; ui::skirmish_shell::state::player_name
app::frontend::skirmish_shell_render::controls -> app::frontend::skirmish_shell_render::chrome; map::scenario_menu; render::batch; render::bit_font; render::skirmish_shell_chrome; rules::color_scheme; ui::shell::geom; ui::skirmish_shell::layout; ui::skirmish_shell::state; ui::skirmish_shell::state::combos; ui::skirmish_shell::state::player_name; ui::skirmish_shell::state::trackbars
app::frontend::skirmish_shell_render::draw_order -> ui::skirmish_shell::layout
app::frontend::skirmish_shell_render::in_game_options -> app::frontend::skirmish_shell_render::controls; assets::csf_file; render::batch; render::bit_font; render::shell_text; render::skirmish_shell_chrome; sidebar::layout_spec; ui::shell::descriptor; ui::shell::geom; ui::shell::in_game_options; ui::shell::in_game_options::control; ui::shell::in_game_options_state; ui::shell::layout
app::frontend::skirmish_shell_render::in_game_shell -> app::frontend::skirmish_shell_render::in_game_options; app::state; render::batch; render::sidebar_chrome; ui::shell::geom; ui::shell::in_game_options; ui::shell::in_game_options::control; ui::shell::in_game_shell; ui::shell::layout
app::frontend::skirmish_shell_render::keyboard -> app::frontend::skirmish_shell_render::chrome; app::frontend::skirmish_shell_render::controls; app::frontend::skirmish_shell_render::in_game_shell; app::frontend::skirmish_shell_render::text; app::input::hotkeys::catalog; app::input::keyboard; app::state; render::batch; render::bit_font; render::shell_paint; render::shell_text; render::shell_text_reveal; render::skirmish_shell_chrome; ui::shell::geom; ui::shell::keyboard; ui::shell::list; ui::shell::static_reveal
app::frontend::skirmish_shell_render::launcher_options -> app::frontend::skirmish_shell_render; app::frontend::skirmish_shell_render::abort; app::frontend::skirmish_shell_render::chrome; app::frontend::skirmish_shell_render::controls; app::frontend::skirmish_shell_render::draw_order; app::frontend::skirmish_shell_render::in_game_options; app::frontend::skirmish_shell_render::in_game_shell; app::frontend::skirmish_shell_render::keyboard; app::frontend::skirmish_shell_render::list; app::frontend::skirmish_shell_render::modals; app::frontend::skirmish_shell_render::pause_menu; app::frontend::skirmish_shell_render::preview; app::frontend::skirmish_shell_render::saved_games; app::frontend::skirmish_shell_render::sound; app::frontend::skirmish_shell_render::text; app::state; map::scenario_menu; render::batch; render::bit_font; render::shell_text; render::shell_text_reveal; render::shell_transition_pass; render::skirmish_shell_chrome; rules::color_scheme; skirmish_modes; ui::main_menu; ui::main_menu_dialogs::options; ui::main_menu_dialogs::options::shell; ui::shell::geom; ui::shell::slide; ui::shell::static_reveal; ui::skirmish_shell::layout; ui::skirmish_shell::state; ui::skirmish_shell::state::player_name
app::frontend::skirmish_shell_render::list -> app::frontend::skirmish_shell_render; app::frontend::skirmish_shell_render::abort; app::frontend::skirmish_shell_render::chrome; app::frontend::skirmish_shell_render::controls; app::frontend::skirmish_shell_render::draw_order; app::frontend::skirmish_shell_render::in_game_options; app::frontend::skirmish_shell_render::in_game_shell; app::frontend::skirmish_shell_render::keyboard; app::frontend::skirmish_shell_render::launcher_options; app::frontend::skirmish_shell_render::modals; app::frontend::skirmish_shell_render::pause_menu; app::frontend::skirmish_shell_render::preview; app::frontend::skirmish_shell_render::saved_games; app::frontend::skirmish_shell_render::sound; app::frontend::skirmish_shell_render::text; app::state; map::scenario_menu; render::batch; render::bit_font; render::shell_transition_pass; render::skirmish_shell_chrome; rules::color_scheme; skirmish_modes; ui::main_menu; ui::shell::geom; ui::shell::list; ui::shell::slide; ui::skirmish_shell::layout; ui::skirmish_shell::state; ui::skirmish_shell::state::player_name
app::frontend::skirmish_shell_render::modals -> app::frontend::skirmish_shell_render::chrome; app::frontend::skirmish_shell_render::controls; app::frontend::skirmish_shell_render::draw_order; render::batch; render::bit_font; render::shell_paint; render::skirmish_shell_chrome; skirmish_modes; ui::shell::geom; ui::skirmish_shell::layout; ui::skirmish_shell::state::player_name; ui::skirmish_shell::state::random_map_setup; ui::skirmish_shell::state::saved_seed_browser; util::native_file_name
app::frontend::skirmish_shell_render::pause_menu -> app::frontend::skirmish_shell_render::in_game_shell; app::state; assets::csf_file; render::bit_font; render::shell_text; sidebar::layout_spec; ui::shell::geom; ui::shell::pause_menu
app::frontend::skirmish_shell_render::preview -> app::frontend::skirmish_shell_render::chrome; app::state; assets::asset_manager; map::preview; map::scenario_menu; render::batch; render::skirmish_shell_chrome; rules::ini_parser; ui::shell::geom
app::frontend::skirmish_shell_render::saved_games -> app::frontend::skirmish_shell_render::in_game_shell; app::frontend::skirmish_shell_render::modals; app::frontend::skirmish_shell_render::pause_menu; app::frontend::skirmish_shell_render::text; app::state; render::shell_text; ui::shell::geom; ui::shell::pause_menu; ui::shell::saved_games; ui::skirmish_shell::layout
app::frontend::skirmish_shell_render::sound -> app::frontend::skirmish_shell_render::controls; app::frontend::skirmish_shell_render::in_game_shell; app::frontend::skirmish_shell_render::pause_menu; app::frontend::skirmish_shell_render::text; app::state; render::shell_paint; render::shell_text; ui::shell::geom; ui::shell::list; ui::shell::pause_menu; ui::shell::sound
app::frontend::skirmish_shell_render::text -> app::frontend::skirmish_shell_render::controls; app::state; map::scenario_menu; render::batch; render::bit_font; render::shell_paint; render::shell_text; ui::main_menu; ui::shell::geom; ui::skirmish_shell::layout; ui::skirmish_shell::state; ui::skirmish_shell::state::combos; ui::skirmish_shell::state::player_name; ui::skirmish_shell::state::random_map_setup; ui::skirmish_shell::state::saved_seed_browser; ui::skirmish_shell::state::trackbars; ui::skirmish_shell::static_reveal
app::frontend::startup_options -> -
app::frontend::startup_splash -> assets::asset_manager; assets::csf_file; assets::fnt_file; assets::pal_file; assets::shp_file; render::batch; render::gpu; render::shell_surface_present
app::frontend::state -> app::frontend::skirmish_session; app::frontend::skirmish_shell_render::launcher_options; app::frontend::startup_splash; app::loading::pump; app::scenario_catalog; app::shell_random_map; app::shell_route; map::scenario_menu; rules::overlay_types; sim::rng; ui::game_screen; ui::main_menu; ui::main_menu_shell::state; ui::score_shell; ui::shell::controller; ui::single_player_shell::state; ui::skirmish_shell::state::player_name; util::legacy_crt_rng
app::handler -> app; app::input::dispatch; app::state; ui::game_screen; ui::shell::controller
app::in_game -> app; app::input::dispatch; app::state; ui::game_screen
app::initialize -> app; app::frontend::list_maps; app::frontend::startup_options; app::frontend::startup_splash; app::persistence::options_profile; app::presentation::render; app::shell_random_map; app::state; app::state::platform; assets::asset_manager; audio::music; audio::sfx; map::basic; map::houses; render::batch; render::bit_font; render::egui_integration; render::gpu; sidebar; sidebar::layout_spec; sim::selection; skirmish_launch; ui::game_screen; util::config
app::input -> -
app::input::abort -> app; app::state; ui::pause_menu; ui::shell::abort
app::input::camera -> app::state; app::types; map::terrain; sim::game_entity
app::input::commands -> app::state; map::entities; net::lockstep; rules::overlay_types; rules::ruleset; sim::command; sim::intern; sim::production; sim::production::production_types; sim::world
app::input::context_order -> app::input::commands; app::input::dispatch; app::input::entity_pick; app::state; app::types; map::entities; rules::object_type; sim::command; sim::intern; sim::world
app::input::cursor -> app::input::commands; app::input::context_order; app::input::entity_pick; app::presentation::instances::helpers; app::state; app::types; render::cursor_atlas; sim::combat; sim::game_entity; sim::world
app::input::dispatch -> app::input::commands; app::input::context_order; app::input::entity_pick; app::input::hotkeys; app::input::sidebar_eva; app::presentation::sidebar_render; app::presentation::target_lines; app::state; app::types; audio::events; map::entities; rules::ruleset; sidebar; sim::command; sim::selection; sim::world
app::input::dispatch::selection_navigation -> app::input::entity_pick; app::input::state; app::state; app::types; assets::csf_file; map::entities
app::input::entity_pick -> app::types; map::entities; rules::ruleset; sim::entity_store; sim::game_entity; sim::intern; sim::vision; sim::world
app::input::gadget_input -> app::presentation::sidebar_render; app::state; sidebar; ui::gadget; ui::gadget::focus; ui::gadget::list; ui::gadget::tick
app::input::gadget_input::command_bar -> app::input::gadget_input; app::presentation::sidebar_render; app::state; sidebar; ui::gadget; ui::gadget::focus; ui::gadget::list; ui::gadget::tick
app::input::hotkeys -> assets::asset_manager; rules::ini_parser
app::input::hotkeys::catalog -> app::input::hotkeys
app::input::in_game_options -> app::state; ui::shell::descriptor; ui::shell::geom; ui::shell::in_game_options; ui::shell::in_game_options::control; ui::shell::in_game_options_state; ui::shell::layout; ui::skirmish_shell::state::trackbars
app::input::keyboard -> app; app::input::hotkeys; app::input::hotkeys::catalog; app::state; ui::shell::keyboard; ui::shell::list
app::input::messages -> app::state; render::batch; render::bit_font; ui::game_screen; ui::messages
app::input::pause_menu -> app; app::state; ui::pause_menu; ui::shell::pause_menu; ui::skirmish_shell::layout
app::input::sidebar_eva -> sim::intern; sim::production::production_types
app::input::sound -> app; app::persistence::options; app::persistence::options::audio; app::state; ui::pause_menu; ui::shell::list; ui::shell::sound
app::input::state -> app::input::camera; app::input::dispatch::selection_navigation; app::input::hotkeys; app::types; sim::selection
app::input::tooltips -> app::presentation::sidebar_render; app::state; assets::csf_file; render::batch; render::bit_font; sidebar; ui::game_screen; ui::tooltips
app::input::transport_orders -> map::entities; rules::object_type; sim::command; sim::game_entity
app::loading -> -
app::loading::composition -> assets::csf_file; map::map_file; map::playfield; map::preview; map::waypoints; skirmish_launch; ui::shell::geom
app::loading::fresh_scenario -> app::frontend::list_maps; app::shell_random_map; map::map_file; map::resolved_terrain; match_bootstrap; sim::scenario_bootstrap
app::loading::init -> app::frontend::list_maps; app::frontend::skirmish; app::loading::fresh_scenario; app::loading::init_helpers; app::loading::pump; app::presentation::lighting; assets::asset_manager; assets::shp_file; map::actions; map::basic; map::cell_tags; map::events; map::houses; map::lighting; map::map_file; map::overlay; map::resolved_terrain; map::scenario_menu; map::tags; map::terrain; map::theater; map::tile_variant_selector; map::trigger_graph; map::triggers; map::waypoints; match_bootstrap; render::batch; render::bridge_atlas; render::bridge_railing_atlas; render::cursor_atlas; render::gpu; render::overlay_atlas; render::sidebar_cameo_atlas; render::sidebar_chrome; render::sprite_atlas; render::tile_atlas; render::unit_atlas; rules::art_data; rules::ini_parser; rules::overlay_types; rules::process_owner; rules::ruleset; rules::tiberium_type; sim::overlay_grid; sim::scenario_bootstrap; sim::trigger_runtime; sim::world; ui::main_menu
app::loading::init_helpers -> app::frontend::skirmish; assets::asset_manager; assets::pal_file; map::basic; map::houses; map::map_file; map::resolved_terrain; map::terrain; map::theater; map::trigger_graph; render::batch; render::gpu; render::sidebar_cameo_atlas; render::sprite_atlas; render::tile_atlas; render::unit_atlas; rules::art_data; rules::ini_parser; rules::native_processing; rules::overlay_types; rules::process_owner; rules::ruleset; sim::scenario_bootstrap; sim::scenario_session; sim::world
app::loading::progress_row -> app::loading::composition; skirmish_launch; ui::shell::geom
app::loading::pump -> app::audio_runtime; app::loading::composition; app::loading::fresh_scenario; app::loading::init; app::loading::progress_row; app::match_runtime::startup; app::process_assets; app::state; assets::asset_manager; assets::pal_file; assets::pcx_file; map::preview; match_bootstrap; render::batch; render::bit_font; render::gpu; render::loading_screen_chrome; render::shell_surface_present; rules::color_scheme; rules::house_colors; sim::scenario_bootstrap; skirmish_launch; ui::game_screen; ui::main_menu
app::loading::pump::render -> app::loading::composition; app::loading::progress_row; app::loading::pump; app::renderer_state; render::batch; render::bit_font; render::draw_state; render::gpu; render::loading_screen_chrome; render::shell_surface_present; render::shell_text; rules::color_scheme; ui::shell::geom
app::loading::transitions -> app::loading::init; app::match_runtime::sim_tick; app::presentation::render; app::state; assets::asset_manager; map::basic; map::houses; map::overlay; map::trigger_graph; render::minimap; render::selection_overlay; rules::overlay_types; rules::sound_ini; sidebar; sim::trigger_runtime; ui::game_screen
app::match_audio -> audio::events
app::match_diagnostics -> sim::replay
app::match_runtime -> -
app::match_runtime::eva_producers -> map::houses; rules::object_type; rules::superweapon_type
app::match_runtime::frame_pacer -> -
app::match_runtime::restore -> app::persistence; app::state
app::match_runtime::scenario_exit -> sim::house_state; ui::score_shell
app::match_runtime::sim_tick -> app::input::commands; app::state; assets::asset_manager; assets::pal_file; map::terrain; render::sprite_atlas; render::unit_atlas; rules::ruleset; sim::command; sim::pathfinding::core; sim::production; sim::replay; sim::trigger_runtime; sim::world; sim::world::lifecycle; ui::game_screen; ui::score_shell
app::match_runtime::sound_dispatch -> app::match_runtime::eva_producers; audio::events; audio::sfx; rules::ruleset; rules::superweapon_type; sim::anim_class; sim::house_state; sim::intern; sim::world
app::match_runtime::startup -> match_bootstrap; sim::world
app::match_runtime::state -> app::input::state; app::match_audio; app::match_diagnostics; app::match_runtime::frame_pacer; app::match_runtime::startup; app::presentation::state; map::basic
app::persistence -> app::persistence::options_profile; map::resolved_terrain; rules::overlay_types; rules::ruleset; sim::ore_growth; sim::production::factory_lifecycle; sim::runtime; sim::snapshot; sim::world
app::persistence::commands -> app::persistence; app::state
app::persistence::keyboard -> app::input::hotkeys
app::persistence::options -> app::persistence::options_profile; app::presentation::target_lines; app::state; ui::shell::in_game_options_state; ui::shell::modal; ui::tooltips
app::persistence::options::audio -> app::state
app::persistence::options::launcher -> app::persistence::options::audio; app::persistence::options_profile; ui::main_menu_dialogs::options
app::persistence::options_profile -> app::frontend::startup_options; rules::ini_parser; util::ini_writer
app::persistence::save_load_panel -> app::persistence; sim::snapshot; ui::client_theme
app::presentation -> -
app::presentation::building_anim -> app::input::commands; app::state; audio::sfx; rules::sound_ini; sim::production
app::presentation::combat_lights -> render::combat_light; rules::ruleset; sim::combat; sim::intern; sim::projectile
app::presentation::fire_effects -> app::state; audio::events; render::wave_geometry; sim::world
app::presentation::instances -> app::presentation::instances::helpers; app::presentation::instances::overlays; app::presentation::instances::particles; app::presentation::instances::shp; app::presentation::instances::units
app::presentation::instances::bridges -> app::presentation::instances::helpers; app::state; map::bridge_facts; map::lighting; map::terrain; render::batch; render::bridge_atlas; render::bridge_railing_atlas; render::draw_state; sim::bridge_state
app::presentation::instances::foot_depth -> app::state; map::entities; map::resolved_terrain; render::foot_depth; rules::object_type; rules::overlay_types; sim::game_entity; sim::movement::locomotor; sim::overlay_grid; sim::runtime; util::native_x87
app::presentation::instances::helpers -> app::state; map::entities; map::terrain; render::batch; render::draw_state; render::native_z; rules::locomotor_type; sim::components; sim::game_entity; sim::intern; sim::vision; sim::world
app::presentation::instances::overlays -> app::presentation::instances::helpers; app::presentation::render::draw_plan_lowering; app::state; map::terrain; render::batch; render::bridge_atlas; render::native_z; render::overlay_atlas; render::palette_light; render::sprite_atlas; render::tactical_draw_plan; rules::art_data; rules::house_colors; rules::overlay_types; sim::anim_class; sim::intern; sim::projectile; sim::world; util::fixed_math
app::presentation::instances::particles -> app::presentation::instances::helpers; app::state; map::terrain; render::batch; render::sprite_atlas; rules::house_colors; rules::particle_system_type; rules::particle_type
app::presentation::instances::shp -> app::presentation::instances::helpers; app::presentation::render::draw_plan_lowering; app::state; map::entities; map::lighting; render::batch; render::draw_state; render::native_z; render::palette_light; render::sprite_atlas; render::tactical_draw_plan; render::unit_atlas; rules::art_data; rules::house_colors; sim::animation; sim::components; sim::game_entity; sim::scenario_session; sim::world
app::presentation::instances::units -> app::presentation::instances::helpers; app::presentation::render::draw_plan_lowering; app::state; map::entities; map::lighting; render::batch; render::draw_state; render::native_z; render::palette_light; render::sprite_atlas; render::tactical_draw_plan; render::unit_atlas; render::unit_slope_transition_cache; rules::house_colors; sim::components; sim::game_entity
app::presentation::lighting -> map::entities; map::lighting; map::resolved_terrain; render::palette_light; rules::ruleset; sim::light_sources; sim::scenario_session; sim::world
app::presentation::overlay_index -> map::overlay
app::presentation::radiation_light -> sim::radiation_light
app::presentation::render -> app::input::commands; app::presentation::render::build_instances; app::state; app::types; render::batch; render::cursor_atlas; sidebar
app::presentation::render::build_instances -> app::diagnostics::debug_overlays; app::input::commands; app::presentation::instances; app::presentation::render; app::presentation::render::draw_plan_lowering; app::presentation::sidebar_build; app::presentation::sidebar_render; app::presentation::ui_overlays; app::state; map::terrain; map::theater; render::batch; render::terrain_instances; sidebar
app::presentation::render::draw_passes -> app::presentation::render::draw_plan_lowering; app::presentation::render::merge_passes; app::presentation::sidebar_render; app::presentation::ui_overlays; app::state; render::batch; render::bridge_atlas; render::overlay_atlas; render::tactical_draw_plan; render::tile_atlas
app::presentation::render::draw_plan_lowering -> render::batch; render::tactical_draw_plan; render::terrain_draw; rules::object_type
app::presentation::render::merge_passes -> app::presentation::render::draw_plan_lowering; render::batch; render::overlay_atlas; render::palette_textures; render::sprite_atlas; render::tactical_draw_plan; render::terrain_draw; render::terrain_draw::batching; render::unit_atlas; render::unit_slope_transition_cache
app::presentation::render::minimap_transaction -> sim::runtime
app::presentation::selection_brackets -> app::input::commands; app::presentation::instances::helpers; app::state; map::entities; render::batch; render::shroud_buffer; sim::components; sim::intern; sim::vision
app::presentation::sidebar_build -> app::presentation::sidebar_render; app::state; render::batch; render::sidebar_chrome; sidebar; sidebar::layout_spec; sidebar::power_bar_anim
app::presentation::sidebar_build::command_bar -> app::presentation::sidebar_build; app::presentation::sidebar_render; app::state; render::batch; render::sidebar_chrome; render::sidebar_chrome::command_bar; sidebar; sidebar::command_bar; sidebar::gadget_flash; sidebar::layout_spec; sidebar::power_bar_anim
app::presentation::sidebar_gadgets -> app::input::commands; app::state; rules::ruleset; sidebar; sim::world
app::presentation::sidebar_render -> app::input::commands; app::presentation::sidebar_build; app::state; assets::csf_file; map::houses; render::batch; sidebar; sidebar::layout_spec; sim::production; sim::production::production_types; sim::superweapon; sim::world
app::presentation::sidebar_text -> render::batch; render::bit_font; render::sidebar_text; sidebar
app::presentation::spawn_pick -> app::frontend::skirmish; app::loading::init_helpers; app::presentation::render; app::state; map::map_file; map::terrain; map::waypoints; ui::game_screen; ui::main_menu
app::presentation::state -> app::input::gadget_input; app::presentation::combat_lights; app::presentation::lighting; app::presentation::overlay_index; app::presentation::target_lines; app::sidebar_projection; map::cell_tags; map::houses; map::overlay; map::tags; map::terrain; map::waypoints; render::bridge_atlas; render::bridge_railing_atlas; render::minimap; render::overlay_atlas; render::selection_overlay; render::sidebar_cameo_atlas; render::sidebar_chrome; render::sprite_atlas; render::tile_atlas; render::unit_atlas; sidebar; sidebar::gadget_flash; sidebar::layout_spec; sidebar::power_bar_anim; ui::messages; ui::pause_menu; ui::shell::abort; ui::shell::button; ui::shell::in_game_options_state; ui::shell::pause_menu; ui::tooltips
app::presentation::target_lines -> map::entities; map::houses; map::terrain; render::batch; rules::house_colors; rules::ruleset; sim::combat; sim::command; sim::components; sim::game_entity; sim::world
app::presentation::ui_overlays -> app::input::commands; app::input::cursor; app::presentation::instances::helpers; app::state; app::types; map::entities; render::batch; render::cursor_atlas; rules::object_type; rules::ruleset; sim::components; sim::intern; sim::vision
app::process_assets -> assets::asset_manager; map::resolved_terrain; map::tile_variant_selector
app::renderer_state -> render::batch; render::bit_font; render::combat_light; render::egui_integration; render::gpu; render::screenshot; render::shell_surface_present; render::terrain_draw
app::scenario_catalog -> map::scenario_menu; map::skirmish_scenarios
app::shell_main_menu -> app; app::audio_runtime; app::diagnostics; app::frame; app::frontend; app::handler; app::in_game; app::initialize; app::input; app::loading; app::loading::transitions; app::match_audio; app::match_diagnostics; app::match_runtime; app::match_runtime::sim_tick; app::persistence; app::presentation; app::presentation::render; app::process_assets; app::renderer_state; app::scenario_catalog; app::shell_random_map; app::shell_route; app::shell_saved_games; app::shell_saved_seeds; app::shell_skirmish; app::sidebar_projection; app::state; app::state::platform; app::types; assets::asset_manager; audio::music; audio::sfx; map::basic; map::houses; map::resolved_terrain; render::batch; render::bit_font; render::egui_integration; render::gpu; rules::overlay_types; sidebar; sidebar::layout_spec; sim::selection; ui::game_screen; ui::main_menu; ui::main_menu_dialogs::options; ui::shell::controller; ui::skirmish_shell::layout; ui::skirmish_shell::state::saved_seed_browser; util::config
app::shell_random_map -> app; app::audio_runtime; app::diagnostics; app::frame; app::frontend; app::frontend::skirmish_session; app::handler; app::in_game; app::initialize; app::input; app::loading; app::loading::transitions; app::match_audio; app::match_diagnostics; app::match_runtime; app::match_runtime::sim_tick; app::persistence; app::presentation; app::presentation::render; app::process_assets; app::renderer_state; app::scenario_catalog; app::shell_main_menu; app::shell_route; app::shell_saved_games; app::shell_saved_seeds; app::shell_skirmish; app::sidebar_projection; app::state; app::state::platform; app::types; assets::asset_manager; audio::music; audio::sfx; map::basic; map::houses; map::map_file; map::resolved_terrain; map::rmg; map::rmg::build; map::rmg::options; map::rmg::preview; map::rmg::settings; map::rmg::theater_blocks; map::theater; map::tile_variant_selector; render::batch; render::bit_font; render::egui_integration; render::gpu; rules::overlay_types; rules::terrain_rules; sidebar; sidebar::layout_spec; sim::rng; sim::selection; ui::game_screen; ui::main_menu; ui::shell::controller; ui::skirmish_shell::layout; ui::skirmish_shell::state::random_map_setup; ui::skirmish_shell::state::saved_seed_browser; util::config
app::shell_route -> -
app::shell_saved_games -> app; app::state; map::rmg::description; ui::pause_menu; ui::shell::list; ui::shell::saved_file_input; ui::skirmish_shell::layout; ui::skirmish_shell::state::saved_seed_browser
app::shell_saved_seeds -> app; app::audio_runtime; app::diagnostics; app::frame; app::frontend; app::handler; app::in_game; app::initialize; app::input; app::loading; app::loading::transitions; app::match_audio; app::match_diagnostics; app::match_runtime; app::match_runtime::sim_tick; app::persistence; app::presentation; app::presentation::render; app::process_assets; app::renderer_state; app::scenario_catalog; app::shell_main_menu; app::shell_random_map; app::shell_route; app::shell_saved_games; app::shell_skirmish; app::sidebar_projection; app::state; app::state::platform; app::types; assets::asset_manager; audio::music; audio::sfx; map::basic; map::houses; map::resolved_terrain; map::rmg::description; map::rmg::saved_seeds; render::batch; render::bit_font; render::egui_integration; render::gpu; rules::overlay_types; sidebar; sidebar::layout_spec; sim::selection; ui::game_screen; ui::main_menu; ui::shell::controller; ui::shell::list; ui::shell::saved_file_input; ui::skirmish_shell::layout; ui::skirmish_shell::state::saved_seed_browser; util::config; util::native_file_name
app::shell_skirmish -> app; app::audio_runtime; app::diagnostics; app::frame; app::frontend; app::handler; app::in_game; app::initialize; app::input; app::loading; app::loading::transitions; app::match_audio; app::match_diagnostics; app::match_runtime; app::match_runtime::sim_tick; app::persistence; app::presentation; app::presentation::render; app::process_assets; app::renderer_state; app::scenario_catalog; app::shell_main_menu; app::shell_random_map; app::shell_route; app::shell_saved_games; app::shell_saved_seeds; app::sidebar_projection; app::state; app::state::platform; app::types; assets::asset_manager; audio::music; audio::sfx; map::basic; map::houses; map::resolved_terrain; render::batch; render::bit_font; render::egui_integration; render::gpu; rules::overlay_types; sidebar; sidebar::layout_spec; sim::selection; ui::game_screen; ui::main_menu; ui::shell::controller; ui::skirmish_shell::layout; ui::skirmish_shell::state::saved_seed_browser; util::config
app::sidebar_projection -> sidebar; sim::intern; sim::world
app::state -> app::audio_runtime; app::diagnostics::state; app::frontend::state; app::match_runtime::state; app::persistence; app::process_assets; app::renderer_state; app::state::platform; map::resolved_terrain; render::egui_integration; rules::overlay_types; ui::main_menu_dialogs::options
app::state::platform -> app::match_runtime::frame_pacer; ui::game_screen
app::types -> render::cursor_atlas
asset_tools -> -
asset_tools::args -> asset_tools::verb_art; asset_tools::verb_compare; asset_tools::verb_csf; asset_tools::verb_extract; asset_tools::verb_find; asset_tools::verb_info; asset_tools::verb_ls; asset_tools::verb_palette; asset_tools::verb_parse_check; asset_tools::verb_render; asset_tools::verb_scan; asset_tools::verb_sound
asset_tools::canvas -> render::bit_font
asset_tools::identify -> assets::format_sniff; assets::hva_file; assets::mix_archive; assets::shp_file; assets::vpl_file
asset_tools::locate -> assets::asset_manager; assets::mix_hash
asset_tools::names -> asset_tools::report; assets::asset_manager; assets::mix_hash; assets::xcc_database
asset_tools::palette -> asset_tools::names; asset_tools::report; assets::asset_manager; assets::pal_file; rules::art_data; rules::ini_parser
asset_tools::palette_production -> -
asset_tools::render_dispatch -> asset_tools::identify; asset_tools::locate; asset_tools::names; asset_tools::render_still; asset_tools::render_tmp; asset_tools::render_vxl; asset_tools::report; asset_tools::verb_render; assets::asset_manager; rules::art_data
asset_tools::render_still -> asset_tools::canvas; asset_tools::identify; asset_tools::palette; asset_tools::report; asset_tools::verb_render; assets::asset_manager; assets::pal_file; assets::pcx_file; rules::color_scheme; rules::house_colors; rules::ini_parser
asset_tools::render_tmp -> asset_tools::canvas; asset_tools::identify; asset_tools::names; asset_tools::palette; asset_tools::report; asset_tools::verb_render; assets::asset_manager; assets::pal_file; assets::tmp_file; rules::art_data
asset_tools::render_vxl -> asset_tools::canvas; asset_tools::identify; asset_tools::names; asset_tools::palette; asset_tools::report; asset_tools::verb_render; assets::asset_manager; assets::hva_file; assets::pal_file; assets::vpl_file; assets::vxl_file; render::unit_atlas; render::vxl_raster; rules::art_data; rules::color_scheme; rules::house_colors; rules::ini_parser
asset_tools::report -> -
asset_tools::root -> assets::asset_manager; util::config
asset_tools::verb_art -> asset_tools::identify; asset_tools::report; assets::asset_manager; rules::art_data
asset_tools::verb_compare -> asset_tools::canvas; asset_tools::identify; asset_tools::names; asset_tools::palette; asset_tools::report; assets::asset_manager; assets::aud_file; assets::csf_file; assets::fnt_file; assets::hva_file; assets::mix_hash; assets::pcx_file; assets::shp_file; assets::tmp_file; assets::vpl_file; assets::vxl_file; rules::art_data
asset_tools::verb_csf -> asset_tools::locate; asset_tools::report; assets::asset_manager; assets::csf_file; util::native_string
asset_tools::verb_extract -> asset_tools::identify; asset_tools::report; assets::asset_manager
asset_tools::verb_find -> asset_tools::identify; asset_tools::names; asset_tools::report; assets::asset_manager; assets::mix_hash
asset_tools::verb_info -> asset_tools::identify; asset_tools::report; assets::asset_manager; assets::aud_file; assets::csf_file; assets::fnt_file; assets::hva_file; assets::pal_file; assets::pcx_file; assets::shp_file; assets::tmp_file; assets::vpl_file; assets::vxl_file
asset_tools::verb_ls -> asset_tools::identify; asset_tools::names; asset_tools::report; assets::asset_manager
asset_tools::verb_palette -> asset_tools::names; asset_tools::palette; asset_tools::report; assets::asset_manager; rules::art_data
asset_tools::verb_parse_check -> asset_tools::identify; asset_tools::names; asset_tools::report; assets::asset_manager; assets::aud_file; assets::csf_file; assets::fnt_file; assets::hva_file; assets::pal_file; assets::pcx_file; assets::shp_file; assets::tmp_file; assets::vpl_file; assets::vxl_file
asset_tools::verb_render -> asset_tools::canvas; asset_tools::identify; asset_tools::names; asset_tools::palette; asset_tools::report; assets::asset_manager; assets::pal_file; assets::shp_file; rules::art_data; rules::color_scheme; rules::house_colors; rules::ini_parser
asset_tools::verb_scan -> asset_tools::identify; asset_tools::names; asset_tools::report; assets::asset_manager; assets::aud_file; assets::csf_file; assets::fnt_file; assets::hva_file; assets::pcx_file; assets::shp_file; assets::tmp_file; assets::vpl_file; assets::vxl_file
asset_tools::verb_sound -> asset_tools::report; assets::asset_manager; assets::audio_bag
assets -> -
assets::asset_manager -> assets::error; assets::mix_archive; assets::mix_hash
assets::aud_file -> assets::ima_adpcm
assets::audio_bag -> -
assets::bink_audio -> assets::bink_bits; assets::bink_file; assets::error
assets::bink_audio_data -> -
assets::bink_bits -> assets::error
assets::bink_data -> -
assets::bink_decode -> assets::bink_bits; assets::bink_file; assets::error
assets::bink_file -> assets::error; util::read_helpers
assets::csf_file -> assets::error; util::read_helpers
assets::error -> -
assets::fnt_file -> -
assets::format_sniff -> assets::mix_archive
assets::hva_file -> assets::error; util::read_helpers
assets::ima_adpcm -> -
assets::mix_archive -> assets::error; assets::mix_crypto; assets::mix_hash; util::read_helpers
assets::mix_crypto -> assets::error
assets::mix_hash -> -
assets::pal_file -> assets::error
assets::pcx_file -> assets::error
assets::shp_decode -> assets::error
assets::shp_file -> assets::error; assets::pal_file; assets::shp_decode; util::read_helpers
assets::tmp_decode -> assets::error; assets::tmp_file; util::read_helpers
assets::tmp_file -> assets::error; assets::pal_file; assets::tmp_decode; util::read_helpers
assets::vpl_file -> assets::error; util::read_helpers
assets::vxl_decode -> assets::error; assets::vxl_file
assets::vxl_file -> assets::error; assets::vxl_decode; util::read_helpers
assets::wav_file -> -
assets::xcc_database -> assets::mix_hash
audio -> -
audio::arbiter -> rules::sound_ini; rules::sound_ini::control
audio::arbiter::event_flags -> -
audio::events -> -
audio::music -> audio::theme
audio::sfx -> assets::asset_manager; assets::aud_file; assets::audio_bag; audio::arbiter; audio::voice_queue; audio::vox; rules::sound_ini; rules::sound_ini::control; rules::sound_ini::sound_type
audio::theme -> assets::asset_manager; assets::aud_file; audio::sfx; rules::ini_parser; sim::rng
audio::voice_queue -> -
audio::vox -> rules::sound_ini
headless_scenario -> assets::asset_manager; map::basic; map::map_file; map::resolved_terrain; map::theater; map::tile_variant_selector; map::waypoints; rules::overlay_types; sim::overlay_grid; sim::runtime; sim::scenario_bootstrap; sim::scenario_session; sim::world; util::pixel_conversion
map -> -
map::actions -> rules::ini_parser
map::authored_overlay -> map::bridge_facts; map::cell_index; map::map_file; map::resolved_terrain; rules::overlay_types; rules::terrain_rules; rules::tiberium_type
map::basic -> rules::ini_parser
map::bridge_facts -> -
map::bridge_pavement -> -
map::bridge_rim_tiles -> map::theater
map::briefing -> rules::ini_parser
map::cell_index -> -
map::cell_tags -> rules::ini_parser
map::construction_trace -> -
map::entities -> rules::ini_parser; rules::mission_data
map::events -> rules::ini_parser
map::houses -> rules::color_scheme; rules::house_colors; rules::ini_parser; rules::ruleset
map::iso_tile_flood -> -
map::lat -> map::map_file; map::theater; rules::ini_parser
map::lighting -> map::entities; rules::art_data; rules::ini_parser; rules::ruleset
map::map_file -> assets::error; assets::mix_archive; map::actions; map::basic; map::briefing; map::cell_tags; map::entities; map::events; map::overlay; map::preview; map::tags; map::trigger_graph; map::triggers; map::tube_facts; map::tubes; map::variable_names; map::waypoints; rules::error; rules::ini_parser; util::base64; util::lzo
map::overlay -> rules::ini_parser; rules::overlay_types; util::base64; util::lcw
map::overlay_types -> rules::overlay_types
map::playfield -> map::cell_index; map::map_file
map::preview -> rules::ini_parser; util::base64; util::lzo
map::resolved_terrain -> assets::asset_manager; assets::tmp_file; map::authored_overlay; map::bridge_facts; map::cell_index; map::lat; map::map_file; map::overlay; map::playfield; map::resolved_terrain::recalc_catalog; map::rmg::preview; map::theater; map::tile_variant_selector; map::tube_facts; map::tubes; rules::overlay_types; rules::terrain_object_type; rules::terrain_rules; util::pixel_conversion
map::resolved_terrain::mutation -> map::resolved_terrain; map::resolved_terrain::zone_class; rules::overlay_types; rules::terrain_rules
map::resolved_terrain::pavement -> assets::tmp_file; map::authored_overlay; map::bridge_facts; map::bridge_pavement; map::cell_index; map::lat; map::map_file; map::overlay; map::playfield; map::resolved_terrain; map::resolved_terrain::mutation; map::resolved_terrain::recalc_catalog; map::resolved_terrain::zone_class; map::rmg::preview; map::theater; map::tile_variant_selector; map::tube_facts; map::tubes; rules::overlay_types; rules::terrain_object_type; rules::terrain_rules; util::pixel_conversion
map::resolved_terrain::recalc_catalog -> assets::asset_manager; assets::tmp_file; map::authored_overlay; map::bridge_facts; map::cell_index; map::lat; map::map_file; map::overlay; map::playfield; map::resolved_terrain; map::resolved_terrain::mutation; map::resolved_terrain::pavement; map::resolved_terrain::zone_class; map::rmg::preview; map::theater; map::tile_variant_selector; map::tube_facts; map::tubes; rules::overlay_types; rules::terrain_object_type; rules::terrain_rules; util::pixel_conversion
map::resolved_terrain::zone_class -> -
map::retail_trig -> util::native_x87
map::rmg -> map::construction_trace; map::map_file; map::rmg::description; map::rmg::grid; map::rmg::options; map::rmg::rng; map::rmg::scratch; map::rmg::settings; map::rmg::tiles; map::rmg::x87; rng_continuation
map::rmg::build -> map::map_file; map::retail_trig; map::rmg; map::rmg::emit; map::rmg::grid; map::rmg::options; map::rmg::phases::shore; map::rmg::phases::tech_buildings; map::rmg::pipeline; map::rmg::rng; map::rmg::scratch; map::rmg::settings; map::rmg::tiles; map::rmg::x87; map::theater; rules::locomotor_type; rules::terrain_rules
map::rmg::description -> -
map::rmg::emit -> map::entities; map::map_file; map::overlay; map::rmg::grid; map::rmg::options; map::waypoints; rules::ini_parser
map::rmg::grid -> -
map::rmg::options -> map::rmg::description; rules::ini_parser; util::ini_writer
map::rmg::phases -> map::rmg::grid
map::rmg::phases::adjacency -> map::rmg::grid; map::rmg::scratch
map::rmg::phases::area -> map::rmg::grid; map::rmg::scratch; map::rmg::tiles
map::rmg::phases::blob -> map::rmg::grid; map::rmg::phases::shore; map::rmg::rng; map::rmg::scratch; map::rmg::tiles; map::rmg::x87
map::rmg::phases::bridge -> map::rmg::phases::blob; map::rmg::phases::meander; map::rmg::phases::shore; map::rmg::x87
map::rmg::phases::bridge_deck -> map::construction_trace; map::rmg::grid; map::rmg::phases::area; map::rmg::phases::carve; map::rmg::rng; map::rmg::scratch; map::rmg::tiles; map::rmg::x87
map::rmg::phases::carve -> map::rmg::grid; map::rmg::phases::connector; map::rmg::phases::ramp; map::rmg::phases::shore; map::rmg::preview; map::rmg::rng; map::rmg::scratch; map::rmg::tiles
map::rmg::phases::carve_driver -> map::construction_trace; map::rmg::phases::adjacency; map::rmg::phases::bridge_deck; map::rmg::phases::carve; map::rmg::rng; map::rmg::x87
map::rmg::phases::connector -> map::rmg::rng; map::rmg::scratch; map::rmg::x87
map::rmg::phases::green_spread -> map::rmg::grid; map::rmg::phases; map::rmg::rng; map::rmg::tiles
map::rmg::phases::hills -> map::rmg::grid; map::rmg::phases::hills_corners; map::rmg::rng; map::rmg::scratch; map::rmg::tiles; map::rmg::x87; map::theater
map::rmg::phases::hills_corners -> map::rmg::grid; map::rmg::scratch; map::rmg::tiles; map::rmg::x87
map::rmg::phases::island_passes -> map::rmg::grid; map::rmg::phases::regions; map::rmg::rng; map::rmg::scratch; map::rmg::tiles; map::rmg::x87
map::rmg::phases::lake -> map::rmg::phases::blob; map::rmg::phases::shore; map::rmg::phases::water; map::rmg::scratch; map::rmg::x87
map::rmg::phases::lat_fixup -> map::lat; map::rmg::grid; map::rmg::tiles
map::rmg::phases::lat_patches -> map::rmg::grid; map::rmg::phases::blob; map::rmg::rng; map::rmg::scratch; map::rmg::tiles; map::rmg::x87
map::rmg::phases::meander -> map::rmg::grid; map::rmg::phases::blob; map::rmg::rng; map::rmg::scratch; map::rmg::x87
map::rmg::phases::ramp -> map::rmg::grid; map::rmg::phases::connector; map::rmg::preview; map::rmg::scratch; map::rmg::tiles
map::rmg::phases::regions -> map::rmg::grid; map::rmg::rng; map::rmg::scratch; map::rmg::tiles
map::rmg::phases::river -> map::rmg::phases::blob; map::rmg::phases::bridge; map::rmg::phases::lake; map::rmg::phases::meander; map::rmg::phases::shore; map::rmg::phases::water; map::rmg::rng; map::rmg::x87
map::rmg::phases::rocks -> map::rmg::grid; map::rmg::rng; map::rmg::scratch; map::rmg::tiles
map::rmg::phases::shore -> map::rmg::grid; map::rmg::rng; map::rmg::scratch; map::rmg::tiles
map::rmg::phases::starts -> map::rmg::grid; map::rmg::phases::area; map::rmg::phases::blob; map::rmg::phases::regions; map::rmg::phases::shore; map::rmg::phases::zones; map::rmg::rng; map::rmg::scratch; map::rmg::tiles; map::rmg::x87
map::rmg::phases::tech_buildings -> map::construction_trace; map::rmg::grid; map::rmg::phases::regions; map::rmg::phases::zones; map::rmg::rng; map::rmg::scratch; map::rmg::tiles
map::rmg::phases::tiberium -> map::rmg::grid; map::rmg::phases::blob; map::rmg::phases::regions; map::rmg::phases::zones; map::rmg::rng; map::rmg::scratch; map::rmg::tiles; map::rmg::x87
map::rmg::phases::trees -> map::rmg::grid; map::rmg::phases::blob; map::rmg::rng; map::rmg::scratch; map::rmg::tiles; map::rmg::x87
map::rmg::phases::water -> map::rmg::grid; map::rmg::phases::blob; map::rmg::phases::lake; map::rmg::phases::shore; map::rmg::rng; map::rmg::x87
map::rmg::phases::water_finalize -> map::rmg::grid; map::rmg::phases::shore; map::rmg::rng; map::rmg::tiles; map::rmg::x87
map::rmg::phases::zones -> map::rmg::grid; map::rmg::phases::shore
map::rmg::pipeline -> map::construction_trace; map::rmg; map::rmg::grid; map::rmg::phases::adjacency; map::rmg::phases::blob; map::rmg::phases::carve; map::rmg::phases::carve_driver; map::rmg::phases::green_spread; map::rmg::phases::hills; map::rmg::phases::island_passes; map::rmg::phases::lat_fixup; map::rmg::phases::lat_patches; map::rmg::phases::regions; map::rmg::phases::rocks; map::rmg::phases::shore; map::rmg::phases::starts; map::rmg::phases::tech_buildings; map::rmg::phases::tiberium; map::rmg::phases::trees; map::rmg::phases::water; map::rmg::phases::water_finalize; map::rmg::phases::zones; map::rmg::preview; map::rmg::rng; map::rmg::scratch; map::rmg::tiles; map::rmg::x87; map::theater
map::rmg::preview -> map::map_file; map::playfield; map::resolved_terrain
map::rmg::randomize -> map::rmg::options; map::rmg::settings
map::rmg::rng -> map::rmg::x87; rng_continuation
map::rmg::saved_seeds -> map::rmg::description; map::rmg::options; util::legacy_crt_rng; util::native_file_name
map::rmg::scratch -> -
map::rmg::settings -> assets::asset_manager; rules::ini_parser
map::rmg::sqrt_table -> -
map::rmg::tech_catalog -> map::rmg::phases::tech_buildings; rules::foundation; rules::ini_parser
map::rmg::theater_blocks -> assets::tmp_file; map::rmg::phases::shore; map::theater
map::rmg::tiles -> map::theater
map::rmg::trig -> map::retail_trig
map::rmg::x87 -> map::rmg::rng
map::scenario_menu -> map::briefing; map::preview; map::waypoints; rules::ini_parser
map::skirmish_scenarios -> map::briefing; map::preview; map::scenario_menu; map::waypoints; rules::ini_parser; skirmish_modes
map::tags -> rules::ini_parser
map::terrain -> map::map_file; map::playfield; map::resolved_terrain
map::theater -> assets::asset_manager; assets::pal_file; assets::tmp_file; map::bridge_facts; map::map_file; rules::ini_parser
map::tile_variant_selector -> -
map::trigger_graph -> map::actions; map::cell_tags; map::events; map::tags; map::triggers
map::triggers -> rules::ini_parser
map::tube_facts -> -
map::tubes -> map::tube_facts; rules::ini_parser
map::variable_names -> rules::ini_parser
map::waypoints -> rules::ini_parser
match_bootstrap -> sim::world; skirmish_launch
net -> -
net::lockstep -> sim::command; sim::intern; sim::world
render -> -
render::batch -> render::draw_state; render::gpu; render::palette_light
render::bink_movie -> assets::bink_decode; assets::bink_file; render::batch; render::gpu
render::bit_font -> assets::fnt_file; render::batch; render::gpu; render::shell_text_reveal
render::bridge_atlas -> assets::asset_manager; assets::pal_file; assets::shp_file; map::overlay; render::batch; render::gpu; render::overlay_atlas; rules::art_data; rules::crate_rules; rules::ini_parser; rules::overlay_types
render::bridge_railing_atlas -> assets::asset_manager; assets::pal_file; assets::shp_file; render::batch; render::gpu
render::building_light -> render::batch
render::building_zshape -> assets::asset_manager; assets::shp_file; render::batch; render::gpu; render::native_z
render::combat_light -> render::batch; render::gpu; sim::projectile
render::current_radar_cell -> map::resolved_terrain; render::minimap_helpers; rules::overlay_types; rules::ruleset; sim::bridge_state; sim::overlay_grid; sim::runtime
render::cursor_atlas -> assets::asset_manager; assets::pal_file; assets::shp_file; render::batch; render::gpu
render::draw_state -> sim::game_entity
render::egui_integration -> render::gpu
render::foot_depth -> util::native_x87
render::frame_readback -> -
render::gpu -> -
render::loading_screen_chrome -> assets::asset_manager; assets::pal_file; assets::pcx_file; assets::shp_file; render::batch; render::gpu
render::locomotor_visual -> rules::locomotor_type; sim::components; sim::game_entity; sim::movement::locomotor
render::main_menu_shell_chrome -> assets::asset_manager; assets::pal_file; assets::shp_file; render::batch; render::gpu
render::minimap -> map::entities; map::houses; map::playfield; map::resolved_terrain; map::terrain; render::batch; render::current_radar_cell; render::gpu; render::minimap_helpers; render::minimap_projection; render::native_radar_surface; render::native_radar_terrain; render::native_radar_viewport; render::radar_events; render::radar_terrain_updates; render::radar_tracker; render::radar_visibility; rules::house_colors; rules::ruleset; sim::entity_store; sim::intern; sim::radar; sim::vision
render::minimap_helpers -> map::houses; map::terrain; render::minimap; rules::house_colors; rules::ruleset; sim::intern; sim::vision
render::minimap_interaction -> map::resolved_terrain; render::batch; render::minimap; render::minimap_projection; render::native_radar_surface; render::native_radar_viewport; render::radar_tracker; sim::entity_store
render::minimap_projection -> map::playfield; map::terrain; render::current_radar_cell; render::minimap; render::minimap_helpers; render::native_radar_surface; render::native_radar_terrain
render::native_radar_surface -> map::playfield; map::resolved_terrain; util::native_x87
render::native_radar_terrain -> render::native_radar_surface; util::native_x87
render::native_radar_viewport -> render::batch; render::native_radar_surface; util::native_x87
render::native_surface_format -> -
render::native_z -> -
render::overlay_assets -> rules::overlay_types
render::overlay_atlas -> assets::asset_manager; assets::pal_file; assets::shp_file; map::overlay; render::batch; render::gpu; render::overlay_assets; rules::art_data; rules::crate_rules; rules::ini_parser; rules::overlay_types; rules::tiberium_type
render::palette_light -> map::lighting
render::palette_textures -> assets::pal_file; render::gpu; rules::house_colors
render::pixel_fx_sparkles -> map::resolved_terrain; map::terrain; render::batch; rules::overlay_types; sim::intern; sim::occupancy; sim::overlay_grid; sim::vision
render::radar_anim -> render::batch; render::gpu; render::radar_animation; render::radar_surface
render::radar_animation -> -
render::radar_events -> render::native_radar_surface; rules::radar_event_config; sim::radar; util::native_x87
render::radar_surface -> render::batch
render::radar_terrain_updates -> render::current_radar_cell; render::minimap_helpers; render::minimap_projection; render::native_radar_surface; render::native_radar_terrain
render::radar_tracker -> map::houses; render::minimap_helpers; render::native_radar_surface; render::radar_visibility; rules::house_colors; rules::ruleset; sim::entity_store; sim::game_entity; sim::intern; sim::vision
render::radar_visibility -> map::entities; render::minimap_helpers; render::radar_tracker; rules::ruleset; sim::combat::veterancy; sim::game_entity; sim::intern; sim::vision
render::screenshot -> -
render::selection_overlay -> assets::asset_manager; assets::pal_file; assets::shp_file; map::terrain; render::batch; render::gpu; render::sprite_atlas; rules::house_colors; sim::production::production_types; sim::selection
render::shell_paint -> render::batch; render::bit_font; render::main_menu_shell_chrome; render::shell_text; render::shell_text_reveal; render::skirmish_shell_chrome; ui::shell::geom
render::shell_surface_present -> render::gpu
render::shell_text -> render::batch; render::bit_font; render::shell_text_reveal
render::shell_text_reveal -> -
render::shell_transition_pass -> -
render::shroud_buffer -> assets::shp_file; map::terrain; render::gpu; sim::intern; sim::vision
render::sidebar_cameo_atlas -> assets::asset_manager; assets::pal_file; assets::shp_file; render::batch; render::gpu; rules::art_data; rules::ruleset
render::sidebar_chrome -> assets::asset_manager; assets::mix_archive; assets::mix_hash; assets::pal_file; assets::shp_file; render::batch; render::gpu; render::sidebar_chrome::command_bar; render::sidebar_chrome::in_game_shell; sidebar::layout_spec
render::sidebar_chrome::command_bar -> assets::asset_manager; assets::mix_archive; assets::mix_hash; assets::pal_file; assets::shp_file; render::batch; render::gpu; render::sidebar_chrome; render::sidebar_chrome::in_game_shell; sidebar::layout_spec
render::sidebar_chrome::in_game_shell -> assets::asset_manager; assets::mix_archive; assets::pal_file; render::sidebar_chrome; sidebar::layout_spec
render::sidebar_text -> render::batch; render::bit_font; sidebar; sidebar::layout_spec
render::skirmish_shell_chrome -> assets::asset_manager; assets::pal_file; assets::pcx_file; assets::shp_file; render::batch; render::gpu
render::smudge -> map::resolved_terrain; map::terrain; render::batch; rules::smudge_type; sim::smudge_grid
render::sprite_atlas -> assets::asset_manager; assets::pal_file; assets::shp_file; map::entities; map::houses; render::batch; render::gpu; rules::art_data; rules::effect_asset_catalog; rules::house_colors; rules::ruleset; sim::entity_store; sim::world
render::tactical_compat -> render::native_surface_format; util::native_x87
render::tactical_draw_plan -> -
render::tactical_shader -> -
render::terrain_draw -> render::batch; render::terrain_draw::batching
render::terrain_draw::batching -> render::terrain_draw
render::terrain_instances -> map::lighting; map::terrain; render::batch; render::draw_state; render::native_z
render::tile_atlas -> map::theater; render::batch; render::gpu
render::unit_atlas -> assets::asset_manager; assets::hva_file; assets::vpl_file; assets::vxl_file; render::batch; render::gpu; render::vxl_compute; render::vxl_raster; rules::art_data; rules::ruleset; sim::components; sim::entity_store; sim::voxel_frame_catalog
render::unit_atlas::shadow_cache -> render::unit_atlas
render::unit_slope_transition_cache -> assets::asset_manager; assets::vpl_file; render::batch; render::gpu; render::unit_atlas; render::vxl_raster; rules::art_data; rules::ruleset; sim::components
render::upscale_pass -> render::gpu
render::vxl_compute -> render::vxl_raster; render::vxl_raster::native
render::vxl_normals -> util::native_x87
render::vxl_raster -> assets::hva_file; assets::vpl_file; assets::vxl_file; render::vxl_normals; render::vxl_raster::native
render::vxl_raster::native -> assets::hva_file; assets::vxl_file; render::vxl_raster; util::native_x87
render::vxl_raster::shadow -> assets::hva_file; assets::vxl_file; render::vxl_raster; render::vxl_raster::native; util::native_x87
render::wave_geometry -> render::batch
rng_continuation -> -
rules -> -
rules::animation_sequence -> rules::ruleset
rules::art_data -> assets::asset_manager; rules::flh; rules::ini_parser; rules::object_type; util::native_x87
rules::bridge_warheads -> rules::ini_parser
rules::color_add -> rules::ini_parser
rules::color_scheme -> rules::ini_parser
rules::combat_damage -> rules::ini_parser
rules::crate_rules -> rules::ini_parser; util::native_x87
rules::effect_asset_catalog -> assets::asset_manager; assets::shp_file; rules::art_data; rules::ruleset
rules::error -> -
rules::flh -> -
rules::foundation -> -
rules::house_colors -> assets::pal_file; rules::color_scheme
rules::infantry_sequence -> rules::animation_sequence; rules::ini_parser
rules::ini_enum -> -
rules::ini_parser -> rules::error
rules::ini_value -> rules::ini_parser
rules::jumpjet_params -> rules::ini_parser; util::fixed_math
rules::locomotor_type -> sim::movement::locomotion::slot
rules::missile_spawn -> rules::ini_parser
rules::missile_spawn::retail_defaults -> -
rules::mission_data -> rules::ini_parser
rules::native_processing -> rules::crate_rules; rules::error; rules::ini_parser; rules::powerups
rules::object_type -> rules::ini_parser; rules::jumpjet_params; rules::locomotor_type; rules::terrain_rules; util::fixed_math; util::native_x87
rules::overlay_types -> rules::ini_parser; rules::terrain_rules; rules::tiberium_type
rules::particle_system_type -> rules::ini_parser; rules::particle_type; util::fixed_math; util::native_x87
rules::particle_type -> rules::ini_parser; util::fixed_math; util::native_x87
rules::powerups -> rules::ini_parser; rules::ini_value; util::native_x87
rules::process_owner -> rules::error; rules::ini_parser; rules::native_processing; rules::ruleset
rules::projectile_type -> rules::ini_parser
rules::radar_event_config -> rules::ini_parser; util::native_x87
rules::ruleset -> assets::asset_manager; rules::art_data; rules::bridge_warheads; rules::color_add; rules::combat_damage; rules::crate_rules; rules::effect_asset_catalog; rules::error; rules::house_colors; rules::ini_parser; rules::missile_spawn; rules::mission_data; rules::native_processing; rules::object_type; rules::overlay_types; rules::particle_system_type; rules::particle_type; rules::powerups; rules::projectile_type; rules::radar_event_config; rules::smudge_type; rules::superweapon_type; rules::terrain_asset_catalog; rules::terrain_object_type; rules::terrain_rules; rules::tiberium_type; rules::voxel_anim_type; rules::warhead_type; rules::weapon_type; util::fixed_math
rules::shp_vehicle_sequence -> rules::animation_sequence; rules::art_data
rules::smudge_type -> rules::ini_parser
rules::sound_ini -> rules::ini_parser; rules::ini_value
rules::sound_ini::control -> -
rules::sound_ini::sound_type -> -
rules::superweapon_type -> rules::ini_parser
rules::team_ai_ini -> rules::ini_parser; rules::ini_value; util::native_x87
rules::terrain_asset_catalog -> assets::asset_manager; assets::shp_file; rules::art_data; rules::ini_parser; rules::ruleset
rules::terrain_object_type -> rules::foundation; rules::ini_parser
rules::terrain_rules -> rules::ini_parser; rules::locomotor_type; util::fixed_math
rules::tiberium_type -> rules::ini_parser
rules::voxel_anim_type -> rules::ini_parser; util::native_x87
rules::warhead_type -> rules::ini_parser; rules::ini_value; util::fixed_math
rules::weapon_type -> rules::ini_parser; util::fixed_math
sidebar -> sidebar::layout_spec; sidebar::power_bar_anim; sidebar::sidebar_view; sim::production::production_types
sidebar::command_bar -> sidebar
sidebar::gadget_flash -> -
sidebar::layout_spec -> -
sidebar::power_bar_anim -> -
sidebar::sidebar_view -> sidebar; sidebar::gadget_flash; sidebar::layout_spec; sim::intern; sim::production::production_types; sim::superweapon
sim -> -
sim::ai -> map::entities; rules::object_type; rules::overlay_types; rules::ruleset; sim::cell_rect; sim::command; sim::intern; sim::pathfinding::core; sim::production; sim::production::production_types; sim::world
sim::ai_buildable -> rules::object_type; rules::ruleset
sim::aircraft -> map::entities; rules::locomotor_type; rules::ruleset; sim::combat; sim::intern; sim::mission::timer; sim::movement::air_movement; sim::movement::locomotor; sim::production::production_tech; sim::world; util::fixed_math
sim::aircraft::attack_mission -> rules::ruleset; sim::aircraft; sim::combat; sim::entity_store; sim::intern; util::fixed_math
sim::aircraft::drop_payload -> map::entities; rules::ruleset; sim::cell_rect; sim::movement::bump_crush; sim::movement::locomotor; sim::movement::parachute_descent; sim::passenger; sim::passenger::departure; sim::pathfinding::core; sim::world; sim::world::lifecycle; util::facing_table; util::fixed_math; util::lepton
sim::aircraft::idle_mode -> sim::aircraft
sim::aircraft::paradrop_mission -> rules::ruleset; sim::aircraft; sim::intern; sim::pathfinding::core; sim::world; sim::world::edge_cell
sim::aircraft::runtime_contract -> rules::locomotor_type; sim::cell_rect
sim::anim_class -> rules::art_data; rules::house_colors; rules::ruleset; sim::components; sim::intern; sim::occupancy; sim::timer; sim::world; sim::world::lifecycle; util::fixed_math; util::lepton
sim::animation -> rules::animation_sequence; sim::entity_store; sim::game_entity; sim::game_options; sim::intern
sim::base_plan -> -
sim::base_plan_generation -> rules::object_type; rules::ruleset; sim::ai_buildable; sim::base_plan; sim::house_state; sim::rng
sim::bounce -> sim::rng; util::native_x87
sim::bridge_specs -> map::bridge_facts; map::bridge_rim_tiles; map::resolved_terrain; sim::bridge_state
sim::bridge_state -> map::bridge_facts; map::resolved_terrain; sim::bridge_specs; sim::bridge_state::damaged_variant; sim::bridge_state::walker; sim::intern; sim::rng
sim::bridge_state::damaged_variant -> map::resolved_terrain; sim::bridge_state
sim::bridge_state::gap_restamp -> map::resolved_terrain; sim::bridge_state
sim::bridge_state::ordinary_repair -> sim::bridge_state::publication; sim::bridge_state::ramp_repair
sim::bridge_state::publication -> map::bridge_facts; sim::bridge_state
sim::bridge_state::ramp_repair -> map::bridge_rim_tiles; sim::bridge_state::publication
sim::bridge_state::record_scan -> map::authored_overlay; map::bridge_facts; map::resolved_terrain; sim::bridge_state; sim::bridge_state::damaged_variant; sim::bridge_state::gap_restamp; sim::bridge_state::ordinary_repair; sim::bridge_state::publication; sim::bridge_state::ramp_repair; sim::bridge_state::repair_occupants; sim::bridge_state::rim; sim::bridge_state::walker; sim::bridge_state::zone_activation; sim::cell_rect
sim::bridge_state::repair_occupants -> sim::bridge_state::publication
sim::bridge_state::rim -> map::bridge_rim_tiles
sim::bridge_state::walker -> map::bridge_facts; map::resolved_terrain; sim::bridge_state; sim::rng
sim::bridge_state::zone_activation -> map::bridge_facts; map::resolved_terrain; sim::bridge_state; sim::bridge_state::damaged_variant; sim::bridge_state::gap_restamp; sim::bridge_state::ordinary_repair; sim::bridge_state::publication; sim::bridge_state::ramp_repair; sim::bridge_state::record_scan; sim::bridge_state::repair_occupants; sim::bridge_state::rim; sim::bridge_state::walker; sim::pathfinding::zone_build
sim::building_art -> rules::art_data; rules::object_type; rules::ruleset; sim::building_art::storage; sim::components; sim::game_entity; sim::world
sim::building_art::admission -> rules::art_data; rules::object_type; rules::ruleset; sim::building_art; sim::building_art::power; sim::building_art::storage; sim::components; sim::game_entity; sim::world
sim::building_art::power -> rules::art_data; rules::ruleset; sim::building_art; sim::world
sim::building_art::storage -> rules::ruleset; sim::building_art; sim::world; util::native_x87; util::native_x87::masked
sim::capture_manager -> rules::object_type; rules::ruleset
sim::cell_kernel -> util::fixed_math; util::lepton
sim::cell_rect -> map::cell_index; map::entities; map::playfield; map::resolved_terrain; map::resolved_terrain::zone_class; rules::locomotor_type; sim::entity_store; sim::movement::locomotor; sim::occupancy; sim::overlay_grid; sim::pathfinding::core; sim::pathfinding::zone_map
sim::cloak_disguise -> sim::cloak_disguise::transitions; sim::intern; sim::rng
sim::cloak_disguise::transitions -> sim::cloak_disguise
sim::combat -> map::entities; map::houses; map::resolved_terrain; rules::animation_sequence; rules::mission_data; rules::object_type; rules::overlay_types; rules::ruleset; rules::warhead_type; rules::weapon_type; sim::bridge_state; sim::combat::combat_aoe; sim::combat::combat_targeting; sim::combat::combat_weapon; sim::combat::damage; sim::combat::line_of_fire; sim::combat::threat_range; sim::combat::veterancy; sim::components; sim::entity_store; sim::game_entity; sim::house_state; sim::house_strategy; sim::infantry; sim::intern; sim::mission::authority; sim::mission::concrete_effects; sim::mission::state; sim::movement::turret; sim::occupancy; sim::overlay_grid; sim::production::production_tech; sim::projectile; sim::rng; sim::terrain_object; sim::vision; sim::wave; sim::world; sim::world::damage_consequences; sim::world::infantry_terminal; util::fixed_math; util::lepton; util::native_x87
sim::combat::base_defense_response -> map::entities; map::houses; map::playfield; map::resolved_terrain; rules::locomotor_type; rules::mission_data; rules::ruleset; sim::cell_rect; sim::combat; sim::combat::base_defense_response::admission; sim::combat::combat_weapon; sim::entity_store; sim::house_state; sim::intern; sim::mission::authority; sim::mission::concrete_effects; sim::mission::state; sim::pathfinding::zone_map; sim::rng; sim::team_script_vm; util::native_x87
sim::combat::base_defense_response::admission -> map::entities; map::resolved_terrain; rules::object_type; rules::ruleset; sim::combat; sim::combat::base_defense_response; sim::combat::combat_weapon; sim::components; sim::entity_store; sim::game_entity; sim::intern; util::fixed_math; util::lepton
sim::combat::cell_spread -> util::fixed_math
sim::combat::combat_aoe -> map::entities; map::resolved_terrain; rules::overlay_types; rules::ruleset; rules::warhead_type; sim::combat; sim::combat::cell_spread; sim::entity_store; sim::game_entity; sim::intern; sim::map::bridge_topology; sim::mission::authority; sim::mission::concrete_effects; sim::movement::locomotor; sim::occupancy; sim::overlay_grid; sim::rng; sim::terrain_object; util::fixed_math; util::lepton; util::native_x87
sim::combat::combat_fire_gate -> map::entities; rules::ruleset; sim::entity_store; sim::intern; sim::movement::teleport_movement; sim::power_system
sim::combat::combat_targeting -> map::entities; map::houses; map::resolved_terrain; rules::object_type; rules::ruleset; sim::combat; sim::combat::combat_weapon; sim::combat::line_of_fire; sim::combat::threat_range; sim::entity_store; sim::game_entity; sim::house_state; sim::intern; sim::occupancy; sim::vision; util::fixed_math; util::native_x87::masked
sim::combat::combat_weapon -> map::entities; map::houses; map::resolved_terrain; rules::animation_sequence; rules::locomotor_type; rules::mission_data; rules::object_type; rules::ruleset; rules::terrain_rules; rules::warhead_type; rules::weapon_type; sim::combat; sim::combat::combat_targeting; sim::deploy; sim::entity_store; sim::game_entity; sim::intern
sim::combat::damage -> -
sim::combat::damage::attacker -> sim::combat::damage
sim::combat::damage::gates -> sim::combat::damage
sim::combat::damage::kernel -> sim::combat::damage; util::native_x87; util::native_x87::masked
sim::combat::damage::receive -> sim::combat::damage; sim::combat::damage::gates; sim::combat::damage::kernel
sim::combat::fire_coord -> map::entities; rules::art_data; rules::object_type; rules::ruleset; rules::weapon_type; sim::combat::combat_targeting; sim::combat::combat_weapon; sim::projectile; sim::world; util::pixel_conversion
sim::combat::fire_decision -> -
sim::combat::greatest_threat -> map::entities; map::houses; map::resolved_terrain; rules::object_type; rules::ruleset; sim::combat; sim::combat::combat_targeting; sim::combat::combat_weapon; sim::combat::line_of_fire; sim::combat::threat_range; sim::entity_store; sim::game_entity; sim::intern; sim::movement::locomotor; sim::occupancy; sim::pathfinding::zone_map; sim::vision; util::fixed_math; util::native_x87; util::native_x87::masked
sim::combat::in_range -> map::cell_index; map::entities; map::resolved_terrain; rules::ruleset; rules::weapon_type; sim::combat; sim::combat::line_of_fire; sim::entity_store; sim::game_entity; sim::intern; sim::production::production_tech; util::fixed_math; util::lepton
sim::combat::inviso_scatter -> sim::rng; util::fixed_math; util::native_x87
sim::combat::line_of_fire -> map::houses; map::resolved_terrain; rules::overlay_types; rules::ruleset; rules::weapon_type; sim::intern; sim::map::bridge_topology; sim::overlay_grid
sim::combat::object_health -> sim::combat::damage; sim::game_entity; util::native_x87; util::native_x87::masked
sim::combat::receiver_health -> map::entities; map::houses; map::resolved_terrain; rules::animation_sequence; rules::mission_data; rules::object_type; rules::overlay_types; rules::ruleset; rules::warhead_type; rules::weapon_type; sim::bridge_state; sim::combat; sim::combat::base_defense_response; sim::combat::cell_spread; sim::combat::combat_aoe; sim::combat::combat_fire_gate; sim::combat::combat_targeting; sim::combat::combat_weapon; sim::combat::damage; sim::combat::fire_coord; sim::combat::fire_decision; sim::combat::greatest_threat; sim::combat::in_range; sim::combat::inviso_scatter; sim::combat::line_of_fire; sim::combat::object_health; sim::combat::smudge_dispatch; sim::combat::threat_range; sim::combat::veterancy; sim::combat::world_receiver; sim::entity_store; sim::game_entity; sim::house_state; sim::house_strategy; sim::infantry; sim::intern; sim::mission::authority; sim::mission::concrete_effects; sim::mission::state; sim::occupancy; sim::overlay_grid; sim::production::production_tech; sim::projectile; sim::rng; sim::terrain_object; sim::vision; sim::wave; sim::world; util::fixed_math; util::lepton; util::native_x87
sim::combat::smudge_dispatch -> map::resolved_terrain; rules::art_data; rules::locomotor_type; rules::smudge_type; sim::combat; sim::combat::inviso_scatter; sim::intern; sim::occupancy; sim::ore_growth; sim::overlay_grid; sim::rng; sim::smudge_grid; sim::tiberium
sim::combat::threat_range -> rules::mission_data; rules::object_type; rules::ruleset; sim::combat::combat_weapon; sim::components; sim::game_entity; util::fixed_math
sim::combat::veterancy -> rules::locomotor_type; rules::object_type; sim::game_entity; util::fixed_math; util::native_x87
sim::combat::world_receiver -> map::entities; map::houses; map::resolved_terrain; rules::animation_sequence; rules::mission_data; rules::object_type; rules::overlay_types; rules::ruleset; rules::warhead_type; rules::weapon_type; sim::bridge_state; sim::combat; sim::combat::base_defense_response; sim::combat::cell_spread; sim::combat::combat_aoe; sim::combat::combat_fire_gate; sim::combat::combat_targeting; sim::combat::combat_weapon; sim::combat::damage; sim::combat::fire_coord; sim::combat::fire_decision; sim::combat::greatest_threat; sim::combat::in_range; sim::combat::inviso_scatter; sim::combat::line_of_fire; sim::combat::object_health; sim::combat::receiver_health; sim::combat::smudge_dispatch; sim::combat::threat_range; sim::combat::veterancy; sim::entity_store; sim::game_entity; sim::house_state; sim::house_strategy; sim::infantry; sim::intern; sim::mission::authority; sim::mission::concrete_effects; sim::mission::state; sim::occupancy; sim::overlay_grid; sim::production::production_tech; sim::projectile; sim::rng; sim::spawn_manager; sim::terrain_object; sim::vision; sim::wave; sim::world; util::fixed_math; util::lepton; util::native_x87
sim::command -> sim::intern; sim::production::production_types
sim::components -> map::entities; sim::anim_class; sim::intern; sim::movement::locomotor; sim::movement::track_process; sim::timer; util::fixed_math; util::native_x87::masked
sim::conversion_health -> rules::object_type; sim::game_entity; util::native_x87
sim::crates -> map::bridge_facts; map::lighting; map::resolved_terrain; rules::crate_rules; rules::locomotor_type; rules::overlay_types; rules::ruleset; rules::terrain_rules; sim::cell_rect; sim::crates::runtime; sim::crates::state; sim::find_nearby_cell; sim::pathfinding::core; sim::rng; sim::world; util::fixed_math
sim::crates::runtime -> map::lighting; rules::crate_rules; rules::overlay_types; rules::ruleset; sim::crates; sim::crates::state; sim::pathfinding::core; sim::world
sim::crates::state -> util::native_x87
sim::crates::state::crate_slot_array_serde -> sim::crates::state
sim::credit_income -> map::entities; rules::ruleset; sim::entity_store; sim::intern; sim::world
sim::debug_event_log -> -
sim::deploy -> sim::entity_store
sim::docking -> -
sim::docking::aircraft_dock -> map::entities; rules::ruleset; sim::intern; sim::movement::locomotor; sim::world
sim::docking::building_dock -> rules::mission_data; rules::ruleset; sim::components; sim::intern; sim::mission::authority; sim::mission::state; sim::mission::timer; sim::movement; sim::movement::locomotor; sim::pathfinding::core; sim::production::production_tech; sim::radio; sim::world; util::fixed_math
sim::docking::bunker_install -> map::entities; rules::ruleset; sim::game_entity; sim::movement; sim::movement::bump_crush; sim::movement::locomotor; sim::occupancy; sim::pathfinding::core; sim::world
sim::docking::bunker_link -> map::entities; rules::mission_data; rules::ruleset; sim::docking::bunker_install; sim::game_entity; sim::mission::authority; sim::mission::state; sim::movement::ground_pose; sim::pathfinding::core; sim::radio; sim::world
sim::docking::pad_geometry -> rules::object_type
sim::economy -> -
sim::entity_store -> sim::game_entity; sim::intern
sim::estimated_health -> -
sim::find_nearby_cell -> map::resolved_terrain; rules::locomotor_type; sim::cell_rect; sim::entity_store; sim::occupancy; sim::overlay_grid; sim::pathfinding::core; sim::pathfinding::zone_map
sim::game_entity -> map::entities; rules::mission_data; sim::aircraft; sim::animation; sim::building_art::storage; sim::cloak_disguise; sim::combat; sim::combat::combat_weapon; sim::components; sim::credit_income; sim::debug_event_log; sim::deploy; sim::docking::aircraft_dock; sim::docking::building_dock; sim::entity_store; sim::estimated_health; sim::intern; sim::miner; sim::mission::leaf; sim::mission::state; sim::mission::timer; sim::movement::drop_pod_movement; sim::movement::locomotor; sim::movement::movement_bridge; sim::movement::rocket_movement; sim::movement::teleport_movement; sim::movement::tube_movement; sim::movement::tunnel_movement; sim::passenger; sim::radio::contacts; sim::slave_miner; sim::superweapon::invulnerability; sim::vision::gap_source; sim::vision::shroud_knowledge; util::native_x87
sim::game_options -> rules::ini_parser
sim::gate_runtime -> map::entities; map::houses; rules::ruleset; sim::entity_store; sim::game_entity; sim::intern; sim::mission::timer; sim::movement::locomotor; sim::occupancy
sim::house_eva -> map::entities; rules::object_type; rules::ruleset; sim::entity_store; sim::game_options; sim::intern; sim::world
sim::house_state -> map::playfield; rules::ruleset; sim::base_plan; sim::economy; sim::intern; sim::world::house_base; util::native_x87
sim::house_strategy -> map::houses; sim::house_state; sim::intern
sim::infantry -> rules::animation_sequence; rules::object_type; rules::ruleset; sim::deploy; sim::entity_store; sim::game_entity; sim::intern; sim::rng; util::fixed_math; util::native_x87::masked
sim::intern -> -
sim::lifecycle_request -> -
sim::light_sources -> map::lighting; rules::ruleset; sim::scenario_session; sim::world
sim::map -> -
sim::map::bridge_occupancy_shadow -> sim::map::bridge_topology
sim::map::bridge_topology -> map::bridge_facts; map::resolved_terrain
sim::mcv_deploy -> map::entities; rules::mission_data; rules::ruleset; sim::game_entity; sim::mission::authority; sim::mission::state; sim::movement; sim::movement::facing_class; sim::world
sim::miner -> rules::object_type; rules::ruleset; sim::miner::harvest_mission; sim::miner::miner_dock_sequence; sim::miner::miner_system; sim::mission::timer; sim::movement::facing_class
sim::miner::harvest_mission -> rules::overlay_types; rules::ruleset; sim::miner; sim::miner::miner_system; sim::pathfinding::core; sim::world
sim::miner::miner_dock -> sim::radio; sim::world
sim::miner::miner_dock_sequence -> map::entities; rules::mission_data; rules::ruleset; sim::components; sim::economy; sim::house_state; sim::miner; sim::miner::miner_dock; sim::miner::miner_system; sim::movement; sim::movement::facing_class; sim::movement::locomotor; sim::occupancy; sim::pathfinding::core; sim::production::production_queue; sim::production::production_tech; sim::world; util::fixed_math
sim::miner::miner_system -> map::entities; rules::locomotor_type; rules::mission_data; rules::object_type; rules::overlay_types; rules::ruleset; rules::tiberium_type; sim::debug_event_log; sim::game_entity; sim::intern; sim::miner; sim::miner::miner_dock; sim::mission::authority; sim::mission::state; sim::movement; sim::movement::locomotor; sim::occupancy; sim::overlay_grid; sim::pathfinding::core; sim::pathfinding::zone_map; sim::production::production_tech; sim::world; util::fixed_math; util::lepton; util::native_x87
sim::mission -> rules::mission_data; sim::mission::leaf; sim::mission::retask; sim::mission::state; sim::mission::timer
sim::mission::authority -> map::entities; rules::ruleset; sim::combat; sim::components; sim::entity_store; sim::game_entity; sim::intern; sim::mission::concrete_effects; sim::mission::readiness; sim::mission::state; sim::mission::verb; sim::world
sim::mission::authority::ready_private -> -
sim::mission::concrete_effects -> sim::combat; sim::components; sim::game_entity; sim::world
sim::mission::concrete_effects::private -> -
sim::mission::control -> rules::mission_data
sim::mission::leaf -> map::entities
sim::mission::readiness -> sim::mission::leaf; sim::mission::state; sim::movement::locomotor_ready
sim::mission::retask -> rules::mission_data; sim::mission::authority; sim::mission::state; sim::world
sim::mission::state -> rules::mission_data; sim::mission::timer
sim::mission::timer -> sim::timer
sim::mission::verb -> sim::mission::state
sim::movement -> map::entities; map::playfield; map::resolved_terrain; rules::locomotor_type; sim::entity_store; sim::game_entity; sim::intern; sim::movement::bump_crush; sim::movement::drive_locomotion; sim::movement::facing_class; sim::movement::locomotion::piggyback; sim::movement::movement_bridge; sim::movement::movement_commands; sim::movement::movement_tick; sim::movement::navcom; sim::pathfinding::cell_entry; sim::pathfinding::core; sim::pathfinding::zone_map; util::fixed_math
sim::movement::air_movement -> rules::locomotor_type; sim::components; sim::debug_event_log; sim::entity_store; sim::movement; sim::movement::locomotor; sim::movement::movement_commands; util::fixed_math
sim::movement::at_coord -> rules::locomotor_type; sim::components; sim::game_entity; sim::movement::drive_track
sim::movement::bump_crush -> map::entities; map::resolved_terrain; rules::ruleset; sim::cell_kernel; sim::entity_store; sim::game_entity; sim::intern; sim::movement::locomotor; sim::movement::movement_commands; sim::occupancy; sim::pathfinding::core; sim::rng; util::fixed_math
sim::movement::cell_arrival -> map::entities; sim::components; sim::movement; sim::movement::bump_crush; sim::movement::locomotor; sim::occupancy; sim::world::substrate
sim::movement::drive_locomotion -> map::resolved_terrain; rules::locomotor_type; sim::components; sim::entity_store; sim::game_entity; sim::pathfinding::terrain_speed; util::fixed_math
sim::movement::drive_track -> sim::components; sim::game_entity
sim::movement::drop_pod_movement -> sim::movement::rocket_movement; util::fixed_math
sim::movement::facing_class -> -
sim::movement::foot_coordinate -> map::resolved_terrain; rules::locomotor_type; sim::components; sim::game_entity; sim::world
sim::movement::foot_mark -> map::entities; rules::overlay_types; rules::ruleset; sim::movement::ground_pose; sim::movement::locomotor; sim::occupancy; sim::pathfinding::core; sim::world
sim::movement::ground_pose -> map::resolved_terrain; rules::object_type; sim::components; sim::game_entity; sim::pathfinding::core; util::lepton
sim::movement::group_destination -> util::direction_tables::lepton; util::native_x87
sim::movement::homing_movement -> sim::entity_store; util::fixed_math
sim::movement::hover -> sim::movement::homing_movement; util::fixed_math
sim::movement::infantry_entry -> map::cell_index; map::resolved_terrain
sim::movement::jumpjet_flight -> map::retail_trig; rules::jumpjet_params; sim::movement::facing_class; util::native_x87
sim::movement::jumpjet_movement -> map::resolved_terrain; rules::jumpjet_params; sim::components; sim::entity_store; sim::intern; sim::movement::jumpjet_flight; sim::movement::locomotor; sim::occupancy; sim::rng; util::fixed_math
sim::movement::locomotion -> sim::movement::locomotion::install; sim::movement::locomotion::piggyback; sim::movement::locomotion::slot
sim::movement::locomotion::install -> rules::locomotor_type; sim::movement::locomotion::slot; sim::substrate::locomotion::class
sim::movement::locomotion::piggyback -> rules::locomotor_type; sim::movement::drop_pod_movement; sim::movement::jumpjet_movement; sim::movement::locomotor; sim::movement::rocket_movement; sim::movement::slope_transition; sim::movement::teleport_movement; sim::movement::tunnel_movement; util::fixed_math
sim::movement::locomotion::power -> -
sim::movement::locomotion::slot -> rules::locomotor_type; sim::substrate::locomotion::class
sim::movement::locomotor -> rules::jumpjet_params; rules::locomotor_type; rules::object_type; sim::movement::locomotion::piggyback; sim::movement::locomotion::slot; sim::movement::slope_transition; util::fixed_math
sim::movement::locomotor_owner -> rules::locomotor_type; sim::components; sim::game_entity; sim::movement::locomotor
sim::movement::locomotor_ready -> -
sim::movement::movement_blocked -> rules::locomotor_type; sim::components; sim::debug_event_log; sim::movement; sim::movement::locomotor; sim::movement::movement_path; sim::movement::path_markers; sim::occupancy; sim::pathfinding::core; sim::pathfinding::terrain_cost; sim::rng; util::fixed_math
sim::movement::movement_bridge -> sim::components; sim::movement::locomotor; sim::pathfinding::core; util::fixed_math
sim::movement::movement_commands -> map::entities; map::resolved_terrain; rules::locomotor_type; rules::ruleset; sim::components; sim::entity_store; sim::game_entity; sim::movement; sim::movement::movement_path; sim::movement::teleport_movement; sim::occupancy; sim::pathfinding::core; sim::pathfinding::terrain_cost; sim::pathfinding::zone_map; util::fixed_math
sim::movement::movement_occupancy -> map::entities; map::houses; map::resolved_terrain; rules::locomotor_type; sim::combat; sim::components; sim::debug_event_log; sim::entity_store; sim::intern; sim::movement; sim::movement::bump_crush; sim::movement::locomotor; sim::movement::movement_blocked; sim::movement::movement_bridge; sim::occupancy; sim::pathfinding::cell_entry; sim::pathfinding::core; sim::pathfinding::terrain_cost; sim::rng
sim::movement::movement_path -> map::resolved_terrain; rules::locomotor_type; sim::components; sim::find_nearby_cell; sim::movement; sim::movement::locomotor; sim::pathfinding::core; sim::pathfinding::path_smooth; sim::pathfinding::terrain_cost; sim::pathfinding::zone_map; sim::pathfinding::zone_search; sim::rng; util::fixed_math
sim::movement::movement_step -> map::entities; map::resolved_terrain; rules::locomotor_type; sim::components; sim::debug_event_log; sim::game_entity; sim::intern; sim::movement; sim::movement::bump_crush; sim::movement::cell_arrival; sim::movement::drive_track; sim::movement::locomotor; sim::movement::movement_blocked; sim::movement::movement_bridge; sim::movement::movement_occupancy; sim::movement::track_process; sim::occupancy; sim::pathfinding::core; sim::pathfinding::terrain_cost; sim::rng; sim::world::substrate; util::fixed_math
sim::movement::movement_tick -> map::entities; map::houses; map::playfield; map::resolved_terrain; rules::locomotor_type; sim::components; sim::debug_event_log; sim::entity_store; sim::game_entity; sim::infantry; sim::intern; sim::lifecycle_request; sim::movement; sim::movement::bump_crush; sim::movement::drive_locomotion; sim::movement::locomotor; sim::movement::movement_blocked; sim::movement::movement_bridge; sim::movement::movement_commands; sim::movement::movement_occupancy; sim::movement::movement_path; sim::movement::movement_step; sim::movement::path_markers; sim::movement::tube_movement; sim::occupancy; sim::pathfinding::cell_entry; sim::pathfinding::core; sim::pathfinding::terrain_cost; sim::pathfinding::terrain_speed; sim::pathfinding::zone_map; sim::rng; sim::type_handle_table; sim::world::substrate; util::fixed_math
sim::movement::navcom -> map::resolved_terrain; rules::locomotor_type; rules::mission_data; sim::components; sim::entity_store; sim::game_entity; util::fixed_math
sim::movement::parachute_descent -> sim::debug_event_log; sim::entity_store; util::fixed_math
sim::movement::path_markers -> map::entities; map::playfield; map::resolved_terrain; rules::locomotor_type; sim::cell_rect; sim::components; sim::entity_store; sim::game_entity; sim::intern; sim::movement::facing_class; sim::movement::locomotor; sim::occupancy; sim::pathfinding::core
sim::movement::ready_producer -> rules::locomotor_type; sim::game_entity; sim::movement::locomotor; sim::movement::locomotor_ready; sim::movement::teleport_movement; util::fixed_math
sim::movement::rocket_movement -> sim::components; sim::debug_event_log; sim::entity_store; sim::intern; sim::movement; sim::movement::locomotion::piggyback; util::fixed_math
sim::movement::scatter -> map::entities; rules::locomotor_type; rules::ruleset; sim::entity_store; sim::intern; sim::movement; sim::movement::bump_crush; sim::movement::locomotor; sim::movement::movement_commands; sim::occupancy; sim::pathfinding::core; sim::pathfinding::terrain_cost; sim::rng; util::fixed_math
sim::movement::slope_transition -> map::entities; rules::locomotor_type; sim::game_entity; sim::movement::locomotion::piggyback; sim::movement::locomotor
sim::movement::teleport_movement -> rules::locomotor_type; rules::ruleset; sim::components; sim::debug_event_log; sim::entity_store; sim::intern; sim::movement::locomotion::piggyback; sim::occupancy; util::fixed_math
sim::movement::track_entry -> rules::overlay_types; rules::ruleset; sim::components; sim::movement; sim::movement::locomotor; sim::movement::movement_occupancy; sim::movement::movement_tick; sim::pathfinding::cell_entry; sim::pathfinding::core; sim::world
sim::movement::track_fresh_dispatch -> -
sim::movement::track_head -> rules::locomotor_type; sim::components; sim::game_entity; sim::movement::drive_track
sim::movement::track_host -> rules::mission_data; rules::overlay_types; rules::ruleset; sim::components; sim::game_entity; sim::lifecycle_request; sim::mission::state; sim::movement::ground_pose; sim::movement::locomotor; sim::movement::track_process; sim::pathfinding::cell_entry; sim::pathfinding::core; sim::world; util::fixed_math
sim::movement::track_process -> sim::components; sim::movement::drive_track; util::native_x87
sim::movement::track_speed -> map::resolved_terrain; rules::locomotor_type; rules::object_type; rules::ruleset; sim::game_entity; sim::pathfinding::core; sim::pathfinding::terrain_speed; util::fixed_math
sim::movement::track_speed_native -> util::native_x87
sim::movement::track_turn -> rules::locomotor_type; rules::mission_data; rules::ruleset; sim::components; sim::game_entity; sim::pathfinding::core; sim::world
sim::movement::tube_movement -> map::entities; map::resolved_terrain; map::retail_trig; map::tube_facts; rules::ruleset; sim::components; sim::entity_store; sim::game_entity; sim::intern; sim::movement::bump_crush; sim::movement::locomotor; sim::movement::movement_commands; sim::occupancy; sim::pathfinding::core; sim::rng; sim::world::substrate; util::lepton; util::native_x87
sim::movement::tunnel_movement -> sim::movement::locomotor; sim::movement::teleport_movement
sim::movement::turret -> rules::ruleset; sim::combat; sim::entity_store; sim::game_entity; sim::intern; util::fixed_math
sim::movement::walk_head -> map::resolved_terrain; sim::components; sim::entity_store; sim::game_entity; sim::intern; sim::occupancy; sim::pathfinding::core; sim::rng; util::fixed_math
sim::movement::walk_host -> rules::overlay_types; rules::ruleset; sim::components; sim::movement::ground_pose; sim::movement::locomotor; sim::pathfinding::core; sim::world
sim::movement::walk_path -> map::playfield; map::resolved_terrain; rules::locomotor_type; rules::mission_data; rules::overlay_types; rules::ruleset; sim::cell_kernel; sim::components; sim::find_nearby_cell; sim::mission::authority; sim::mission::concrete_effects; sim::mission::state; sim::movement::ground_pose; sim::movement::infantry_entry; sim::movement::movement_tick; sim::pathfinding::core; sim::pathfinding::zone_map; sim::pathfinding::zone_search; sim::world
sim::multiplayer_checksum -> map::entities; sim::game_entity; sim::intern; sim::wave; sim::world; util::native_x87
sim::native_identity -> map::tubes; rules::ini_parser; sim::world
sim::naval_base_placement -> map::entities; map::playfield; map::resolved_terrain; rules::locomotor_type; rules::object_type; rules::ruleset; sim::find_nearby_cell; sim::house_state; sim::intern; sim::pathfinding::core; sim::world
sim::occupancy -> map::cell_index; map::entities; map::resolved_terrain; rules::object_type; sim::components; sim::entity_store; sim::game_entity; sim::intern; sim::movement::locomotor; util::fixed_math
sim::ore_growth -> map::authored_overlay; map::basic; map::resolved_terrain; rules::overlay_types; rules::tiberium_type; sim::overlay_grid; sim::rng; sim::scenario_session; sim::tiberium; util::native_x87
sim::ore_twinkle -> map::authored_overlay; rules::overlay_types; rules::ruleset; rules::tiberium_type; sim::anim_class; sim::components; sim::world; util::fixed_math; util::lepton
sim::overlay_grid -> map::authored_overlay; map::overlay; map::resolved_terrain; rules::overlay_types; sim::intern; sim::rng; util::lepton; util::native_x87
sim::parity_digest -> sim::entity_store; sim::house_state; sim::intern
sim::particles -> rules::particle_system_type; rules::particle_type; sim::intern; sim::world; util::fixed_math; util::native_x87
sim::particles::fire -> rules::particle_type; rules::ruleset; sim::particles; sim::particles::spawn; sim::rng; sim::world; util::fixed_math
sim::particles::gas -> rules::particle_type; rules::ruleset; sim::particles; sim::rng; sim::world; util::fixed_math
sim::particles::ivec3_serde -> -
sim::particles::smoke -> rules::particle_type; rules::ruleset; sim::particles; sim::rng; sim::world; util::fixed_math
sim::particles::spark -> sim::particles; sim::rng; util::native_x87
sim::particles::spark_spawn -> rules::particle_type; rules::ruleset; sim::particles; sim::particles::spark_world; sim::rng; sim::world; util::fixed_math; util::native_x87
sim::particles::spark_world -> map::entities; map::resolved_terrain; rules::foundation; rules::ruleset; sim::cell_rect; sim::movement::locomotor; sim::particles::spark; sim::world; util::lepton; util::native_x87
sim::particles::spawn -> rules::object_type; rules::particle_system_type; rules::particle_type; rules::ruleset; sim::intern; sim::particles; sim::rng; sim::world; util::fixed_math; util::native_x87
sim::particles::system_ai -> rules::particle_system_type; rules::particle_type; rules::ruleset; sim::particles; sim::particles::spark; sim::particles::spark_world; sim::world
sim::particles::wind -> -
sim::passenger -> rules::object_type; rules::ruleset; sim::components; sim::game_entity; sim::house_state; sim::intern; sim::movement; sim::passenger::departure; sim::pathfinding::core; sim::world; sim::world::lifecycle; util::fixed_math
sim::passenger::departure -> rules::ruleset; sim::movement::locomotor; sim::passenger; sim::world; sim::world::lifecycle; util::lepton
sim::pathfinding -> sim::pathfinding::core
sim::pathfinding::cell_entry -> map::entities; map::houses; map::resolved_terrain; map::resolved_terrain::zone_class; rules::locomotor_type; sim::cell_rect; sim::entity_store; sim::game_entity; sim::intern; sim::movement::bump_crush; sim::movement::locomotor; sim::occupancy; sim::pathfinding::core; sim::pathfinding::terrain_cost
sim::pathfinding::core -> map::map_file; map::resolved_terrain; map::theater; map::tube_facts; rules::locomotor_type; sim::bridge_state; sim::movement::locomotor; sim::pathfinding::cell_entry; sim::pathfinding::terrain_cost; sim::pathfinding::zone_hierarchy; sim::pathfinding::zone_map
sim::pathfinding::passability -> rules::locomotor_type; rules::terrain_rules
sim::pathfinding::path_smooth -> sim::movement::locomotor
sim::pathfinding::terrain_cost -> map::resolved_terrain; rules::locomotor_type
sim::pathfinding::terrain_speed -> map::resolved_terrain; rules::locomotor_type; util::fixed_math
sim::pathfinding::zone_build -> map::resolved_terrain; map::resolved_terrain::zone_class; rules::locomotor_type; rules::terrain_rules; sim::bridge_state; sim::movement::locomotor; sim::pathfinding::core; sim::pathfinding::passability; sim::pathfinding::terrain_cost; sim::pathfinding::zone_hierarchy; sim::pathfinding::zone_map; util::native_x87
sim::pathfinding::zone_hierarchy -> rules::locomotor_type; sim::pathfinding::passability; sim::pathfinding::zone_map
sim::pathfinding::zone_incremental -> map::resolved_terrain; map::resolved_terrain::zone_class; rules::locomotor_type; sim::bridge_state; sim::movement::locomotor; sim::pathfinding::core; sim::pathfinding::terrain_cost; sim::pathfinding::zone_build; sim::pathfinding::zone_hierarchy; sim::pathfinding::zone_map
sim::pathfinding::zone_map -> map::cell_index; map::playfield; map::resolved_terrain; rules::locomotor_type; rules::terrain_rules; sim::bridge_state; sim::movement::locomotor; sim::pathfinding::core; sim::pathfinding::terrain_cost; sim::pathfinding::zone_build; sim::pathfinding::zone_hierarchy
sim::pathfinding::zone_map::bridge_repair_zones -> map::playfield; map::resolved_terrain; rules::locomotor_type; sim::bridge_state; sim::cell_rect; sim::pathfinding::zone_hierarchy; sim::pathfinding::zone_map
sim::pathfinding::zone_search -> map::playfield; map::resolved_terrain; map::tube_facts; rules::locomotor_type; sim::cell_rect; sim::movement::locomotor; sim::pathfinding::core; sim::pathfinding::terrain_cost; sim::pathfinding::zone_hierarchy; sim::pathfinding::zone_map
sim::power_system -> map::entities; rules::ruleset; sim::entity_store; sim::game_entity; sim::intern
sim::production -> sim::production::factory; sim::production::factory_lifecycle; sim::production::production_economy; sim::production::production_placement; sim::production::production_queue; sim::production::production_refinery; sim::production::production_sell; sim::production::production_spawn; sim::production::production_tech; sim::production::production_types; sim::production::war_factory_exit
sim::production::factory -> rules::object_type; rules::ruleset; sim::economy; sim::intern; sim::production::production_tech; sim::production::production_types; sim::world
sim::production::factory_lifecycle -> rules::object_type; rules::ruleset; sim::intern; sim::production::factory; sim::production::production_queue; sim::production::production_tech; sim::production::production_types; sim::world
sim::production::production_economy -> rules::ruleset; sim::miner; sim::pathfinding; sim::world
sim::production::production_placement -> map::entities; map::houses; rules::locomotor_type; rules::object_type; rules::overlay_types; rules::ruleset; sim::components; sim::entity_store; sim::intern; sim::movement::locomotor; sim::pathfinding; sim::production::production_tech; sim::production::production_types; sim::production::wall_placement; sim::world
sim::production::production_queue -> rules::ruleset; sim::intern; sim::production::factory_lifecycle; sim::production::production_economy; sim::production::production_spawn; sim::production::production_tech; sim::production::production_types; sim::world
sim::production::production_refinery -> map::entities; rules::ruleset; sim::find_nearby_cell; sim::pathfinding::core; sim::production::production_tech; sim::world; sim::world::lifecycle
sim::production::production_sell -> map::entities; rules::mission_data; rules::object_type; rules::ruleset; sim::combat; sim::components; sim::intern; sim::movement; sim::movement::locomotor; sim::passenger; sim::pathfinding::cell_entry; sim::production::production_queue; sim::production::production_tech; sim::world; sim::world::lifecycle; util::lepton
sim::production::production_spawn -> map::resolved_terrain; rules::locomotor_type; rules::object_type; rules::ruleset; sim::entity_store; sim::movement::bump_crush; sim::movement::locomotor; sim::occupancy; sim::pathfinding::core; sim::production::production_tech; sim::production::production_types; sim::world
sim::production::production_tech -> map::entities; rules::object_type; rules::ruleset; sim::entity_store; sim::intern; sim::production::factory; sim::production::production_queue; sim::production::production_types; sim::world
sim::production::production_types -> rules::object_type; sim::docking::aircraft_dock; sim::intern; sim::ore_growth; sim::production::factory
sim::production::wall_placement -> rules::object_type; rules::overlay_types; rules::ruleset; sim::intern; sim::overlay_grid; sim::pathfinding::core; sim::production::production_placement; sim::world
sim::production::war_factory_exit -> map::entities; rules::ruleset; sim::entity_store; sim::intern; sim::movement::locomotor; sim::occupancy; sim::production::production_spawn
sim::projectile -> map::resolved_terrain; sim::intern; sim::movement::homing_movement; sim::rng; sim::timer; util::fixed_math; util::native_x87
sim::projectile::launch -> map::retail_trig; sim::projectile; util::native_x87
sim::radar -> rules::ruleset; sim::world
sim::radiation -> map::resolved_terrain; rules::ruleset; util::lepton
sim::radiation_light -> map::lighting; rules::ruleset; sim::radiation; sim::world; util::fixed_math
sim::radio -> map::entities; sim::radio::contacts; sim::radio::receive; sim::world
sim::radio::contacts -> -
sim::radio::receive -> map::entities; sim::docking::bunker_install; sim::radio; sim::world
sim::replay -> sim::command; sim::runtime; util::pixel_conversion
sim::rng -> rng_continuation
sim::rocking -> sim::rocking::impulse; sim::rocking::rocking_system; sim::rocking::self_destruct
sim::rocking::impulse -> sim::components; util::fixed_math
sim::rocking::rocking_system -> map::entities; rules::ruleset; sim::components; sim::entity_store; sim::game_entity; sim::rocking::self_destruct; util::fixed_math
sim::rocking::self_destruct -> sim::game_entity; util::fixed_math
sim::runtime -> assets::asset_manager; map::authored_overlay; map::basic; map::entities; map::houses; map::map_file; map::resolved_terrain; map::theater; map::trigger_graph; rules::art_data; rules::overlay_types; rules::ruleset; sim::anim_class; sim::command; sim::components; sim::entity_store; sim::game_entity; sim::intern; sim::native_identity; sim::overlay_grid; sim::scenario_bootstrap; sim::scenario_post_map; sim::scenario_session; sim::vision; sim::world; sim::world::authored_load_host; sim::world::world_spawn
sim::scenario_bootstrap -> map::construction_trace; map::entities; map::houses; map::map_file; map::playfield; map::resolved_terrain; map::waypoints; rng_continuation; rules::ini_parser; rules::locomotor_type; rules::mission_data; rules::object_type; rules::overlay_types; rules::process_owner; rules::ruleset; sim::ai; sim::find_nearby_cell; sim::house_state; sim::mission::state; sim::native_identity; sim::rng; sim::scenario_session; sim::world; sim::world::lifecycle; skirmish_launch; util::native_x87
sim::scenario_post_map -> map::basic; map::houses; map::lighting; rules::overlay_types; rules::ruleset; sim::crates; sim::ore_growth; sim::ore_twinkle; sim::world
sim::scenario_session -> sim::game_options; sim::intern; sim::replay; sim::timer; util::pixel_conversion
sim::score -> sim::intern; sim::world
sim::selection -> sim::entity_store
sim::sensor_lifecycle -> map::entities; rules::object_type; rules::ruleset; sim::intern; sim::movement::locomotor; sim::world
sim::slave_deposit -> map::cell_index; map::entities; map::resolved_terrain; rules::ruleset; sim::components; sim::entity_store; sim::intern; sim::movement::ground_pose; sim::movement::locomotor; sim::occupancy
sim::slave_miner -> rules::ruleset; sim::economy; sim::house_state; sim::intern; sim::miner; sim::miner::miner_system; sim::pathfinding::core; sim::production::production_queue; sim::world; sim::world::lifecycle
sim::smudge_grid -> map::map_file; map::resolved_terrain; rules::smudge_type; sim::occupancy; sim::overlay_grid; sim::rng
sim::snapshot -> sim::components; sim::intern; sim::ore_growth; sim::world
sim::spawn_manager -> rules::missile_spawn; rules::object_type; rules::ruleset; sim::combat; sim::intern; sim::world; sim::world::lifecycle
sim::substrate -> -
sim::substrate::locomotion -> sim::substrate::locomotion::capability; sim::substrate::locomotion::class; sim::substrate::locomotion::defaults
sim::substrate::locomotion::capability -> sim::substrate::locomotion::class
sim::substrate::locomotion::class -> sim::movement::locomotion::slot
sim::substrate::locomotion::defaults -> sim::substrate::locomotion::class
sim::superweapon -> rules::ruleset; rules::superweapon_type; sim::intern; sim::timer; sim::world
sim::superweapon::cell_grid -> sim::cell_rect; sim::movement::locomotor; sim::world
sim::superweapon::force_shield -> map::entities; map::houses; rules::ruleset; sim::intern; sim::superweapon::invulnerability; sim::world
sim::superweapon::genetic_converter -> rules::overlay_types; rules::ruleset; sim::intern; sim::world
sim::superweapon::genetic_converter::mutation -> map::entities; rules::overlay_types; rules::ruleset; sim::combat::combat_aoe; sim::intern; sim::superweapon::cell_grid; sim::world
sim::superweapon::invulnerability -> sim::game_entity
sim::superweapon::iron_curtain -> map::entities; rules::ruleset; sim::intern; sim::superweapon::cell_grid; sim::superweapon::invulnerability; sim::world
sim::superweapon::lightning_storm -> rules::overlay_types; rules::ruleset; sim::combat::combat_aoe; sim::intern; sim::world
sim::superweapon::paradrop -> rules::ruleset; sim::aircraft; sim::intern; sim::movement::air_movement; sim::movement::locomotor; sim::passenger; sim::pathfinding::core; sim::world; sim::world::edge_cell; sim::world::lifecycle; util::fixed_math
sim::superweapon::psychic_reveal -> rules::ruleset; sim::intern; sim::vision; sim::world
sim::team_script_vm -> rules::locomotor_type; rules::object_type; rules::ruleset; rules::team_ai_ini; sim::command; sim::intern; util::native_x87
sim::team_script_vm::registry_install -> rules::locomotor_type; rules::ruleset; rules::team_ai_ini; sim::intern; sim::pathfinding::passability; sim::team_script_vm
sim::terrain_object -> map::resolved_terrain; rules::ruleset; rules::terrain_object_type; rules::warhead_type; sim::combat; sim::combat::damage; sim::intern; sim::occupancy; sim::production::production_types; sim::terrain_spawn
sim::terrain_spawn -> map::overlay; map::resolved_terrain; rules::overlay_types; rules::ruleset; rules::tiberium_type; sim::entity_store; sim::intern; sim::occupancy; sim::ore_growth; sim::overlay_grid; sim::rng; sim::terrain_object; sim::tiberium; sim::world
sim::tiberium -> map::entities; map::resolved_terrain; rules::overlay_types; rules::ruleset; rules::tiberium_type; sim::entity_store; sim::intern; sim::miner; sim::occupancy; sim::ore_growth; sim::overlay_grid; sim::rng
sim::tiberium_germinate -> map::authored_overlay; map::cell_index; map::resolved_terrain; rules::overlay_types; rules::tiberium_type; sim::ore_twinkle; sim::overlay_grid
sim::timer -> -
sim::transport_unload -> map::entities; rules::locomotor_type; rules::mission_data; rules::ruleset; rules::terrain_rules; sim::cell_rect; sim::find_nearby_cell; sim::game_entity; sim::mission::authority; sim::mission::retask; sim::mission::state; sim::movement::bump_crush; sim::movement::facing_class; sim::movement::locomotor; sim::movement::ready_producer; sim::passenger::departure; sim::pathfinding::core; sim::world
sim::trigger_runtime -> map::actions; map::events; map::trigger_graph; map::triggers; map::variable_names; sim::world
sim::type_handle_table -> rules::ruleset; sim::intern
sim::vision -> map::houses; sim::entity_store; sim::game_entity; sim::intern; sim::pathfinding::core; sim::vision::gap_source; sim::vision::shroud_knowledge
sim::vision::gap_source -> sim::intern
sim::vision::map_reveal -> sim::intern; sim::vision
sim::vision::shroud_knowledge -> sim::intern; sim::timer
sim::voxel_anim -> rules::voxel_anim_type; sim::bounce; sim::intern; sim::rng; util::native_x87
sim::voxel_frame_catalog -> assets::asset_manager; assets::hva_file; rules::art_data; rules::ruleset; sim::components; sim::entity_store; sim::intern
sim::wave -> map::resolved_terrain; map::retail_trig; sim::cell_rect; sim::combat; sim::intern; sim::projectile; util::native_x87
sim::world -> map::actions; map::bridge_facts; map::cell_index; map::construction_trace; map::entities; map::events; map::houses; map::lighting; map::map_file; map::overlay; map::playfield; map::resolved_terrain; map::trigger_graph; map::triggers; map::tubes; rules::art_data; rules::ini_parser; rules::locomotor_type; rules::mission_data; rules::object_type; rules::overlay_types; rules::particle_system_type; rules::ruleset; rules::team_ai_ini; rules::warhead_type; sim::ai; sim::anim_class; sim::animation; sim::bridge_state; sim::cell_rect; sim::combat; sim::combat::combat_aoe; sim::combat::combat_weapon; sim::combat::damage; sim::command; sim::components; sim::crates::state; sim::docking::aircraft_dock; sim::docking::building_dock; sim::entity_store; sim::game_entity; sim::house_state; sim::house_strategy; sim::intern; sim::lifecycle_request; sim::light_sources; sim::mission::authority; sim::mission::retask; sim::mission::state; sim::movement; sim::movement::air_movement; sim::movement::drop_pod_movement; sim::movement::group_destination; sim::movement::infantry_entry; sim::movement::locomotor; sim::movement::movement_tick; sim::movement::rocket_movement; sim::movement::teleport_movement; sim::movement::track_process; sim::movement::track_turn; sim::movement::tunnel_movement; sim::movement::turret; sim::multiplayer_checksum; sim::occupancy; sim::ore_growth; sim::ore_twinkle; sim::overlay_grid; sim::parity_digest; sim::particles; sim::passenger; sim::pathfinding::core; sim::pathfinding::terrain_cost; sim::pathfinding::terrain_speed; sim::pathfinding::zone_incremental; sim::pathfinding::zone_map; sim::power_system; sim::production; sim::production::production_types; sim::projectile; sim::radar; sim::radiation; sim::rng; sim::scenario_bootstrap; sim::scenario_post_map; sim::scenario_session; sim::team_script_vm; sim::tiberium; sim::trigger_runtime; sim::type_handle_table; sim::vision; sim::wave; sim::world::frame_error; sim::world::hash_schema; sim::world::house_base; sim::world::infantry_terminal; sim::world::lifecycle; sim::world::load_object_lifecycle; sim::world::logic_vector; sim::world::substrate; sim::world::techno_ai; sim::world::techno_ai::mission_handlers; sim::world::world_orders; sim::world::world_spawn; sim::world::world_spawn::construction; util::fixed_math
sim::world::authored_load_host -> assets::asset_manager; map::authored_overlay; map::cell_index; map::resolved_terrain; rules::art_data; sim::anim_class; sim::components; sim::world; sim::world::load_object_lifecycle
sim::world::bridge_hut_scatter -> map::cell_index; rules::locomotor_type; rules::overlay_types; rules::ruleset; sim::components; sim::find_nearby_cell; sim::movement; sim::movement::locomotor; sim::world; sim::world::frame_error
sim::world::bridge_orchestrator -> map::bridge_facts; map::resolved_terrain; rules::overlay_types; rules::ruleset; sim::bridge_state; sim::components; sim::intern; sim::rng; sim::world; sim::world::bridge_orchestrator::live_publication::repair_publication; util::fixed_math
sim::world::bridge_orchestrator::ground_fallout -> rules::overlay_types; rules::ruleset; sim::combat; sim::movement::locomotor; sim::occupancy; sim::world
sim::world::bridge_orchestrator::live_publication -> map::bridge_facts; map::cell_index; map::resolved_terrain; rules::ruleset; sim::bridge_state; sim::bridge_state::publication; sim::intern; sim::rng; sim::world; sim::world::bridge_orchestrator; sim::world::bridge_orchestrator::ground_fallout; sim::world::bridge_orchestrator::live_publication::repair_publication; util::fixed_math
sim::world::bridge_orchestrator::live_publication::constructor_publication -> map::bridge_facts; map::cell_index; map::resolved_terrain; rules::ruleset; sim::bridge_state; sim::bridge_state::publication; sim::intern; sim::rng; sim::world; sim::world::bridge_orchestrator; sim::world::bridge_orchestrator::ground_fallout; sim::world::bridge_orchestrator::live_publication; sim::world::bridge_orchestrator::live_publication::pavement_publication; sim::world::bridge_orchestrator::live_publication::repair_publication; sim::world::bridge_orchestrator::live_publication::rim_publication; sim::world::bridge_orchestrator::live_publication::tile_publication; sim::world::bridge_orchestrator::live_publication::zone_publication; sim::world::load_object_lifecycle; util::fixed_math
sim::world::bridge_orchestrator::live_publication::pavement_publication -> map::bridge_facts; map::bridge_pavement; map::cell_index; map::resolved_terrain; rules::ruleset; sim::bridge_state; sim::bridge_state::publication; sim::intern; sim::rng; sim::world; sim::world::bridge_orchestrator; sim::world::bridge_orchestrator::ground_fallout; sim::world::bridge_orchestrator::live_publication; sim::world::bridge_orchestrator::live_publication::constructor_publication; sim::world::bridge_orchestrator::live_publication::repair_publication; sim::world::bridge_orchestrator::live_publication::rim_publication; sim::world::bridge_orchestrator::live_publication::tile_publication; sim::world::bridge_orchestrator::live_publication::zone_publication; util::fixed_math
sim::world::bridge_orchestrator::live_publication::repair_publication -> map::bridge_facts; map::bridge_rim_tiles; map::cell_index; map::resolved_terrain; rules::ruleset; sim::bridge_state; sim::bridge_state::ordinary_repair; sim::bridge_state::publication; sim::bridge_state::ramp_repair; sim::intern; sim::rng; sim::world; sim::world::bridge_orchestrator; sim::world::bridge_orchestrator::ground_fallout; sim::world::bridge_orchestrator::live_publication; sim::world::bridge_orchestrator::live_publication::constructor_publication; sim::world::bridge_orchestrator::live_publication::pavement_publication; sim::world::bridge_orchestrator::live_publication::rim_publication; sim::world::bridge_orchestrator::live_publication::tile_publication; sim::world::bridge_orchestrator::live_publication::zone_publication; util::fixed_math
sim::world::bridge_orchestrator::live_publication::repair_publication::occupants -> map::bridge_facts; map::bridge_rim_tiles; map::cell_index; map::entities; map::resolved_terrain; rules::ruleset; sim::bridge_state; sim::bridge_state::ordinary_repair; sim::bridge_state::publication; sim::bridge_state::ramp_repair; sim::bridge_state::repair_occupants; sim::combat; sim::intern; sim::movement::at_coord; sim::movement::locomotor; sim::occupancy; sim::rng; sim::world; sim::world::bridge_orchestrator; sim::world::bridge_orchestrator::ground_fallout; sim::world::bridge_orchestrator::live_publication; sim::world::bridge_orchestrator::live_publication::constructor_publication; sim::world::bridge_orchestrator::live_publication::pavement_publication; sim::world::bridge_orchestrator::live_publication::repair_publication; sim::world::bridge_orchestrator::live_publication::rim_publication; sim::world::bridge_orchestrator::live_publication::tile_publication; sim::world::bridge_orchestrator::live_publication::zone_publication; util::fixed_math
sim::world::bridge_orchestrator::live_publication::repair_publication::occupants::admission -> map::bridge_facts; map::bridge_rim_tiles; map::cell_index; map::entities; map::resolved_terrain; rules::locomotor_type; rules::mission_data; rules::object_type; rules::ruleset; sim::bridge_state; sim::bridge_state::ordinary_repair; sim::bridge_state::publication; sim::bridge_state::ramp_repair; sim::bridge_state::repair_occupants; sim::combat; sim::combat::combat_weapon; sim::components; sim::game_entity; sim::intern; sim::movement::at_coord; sim::movement::bump_crush; sim::movement::infantry_entry; sim::movement::locomotor; sim::occupancy; sim::rng; sim::world; sim::world::bridge_orchestrator; sim::world::bridge_orchestrator::ground_fallout; sim::world::bridge_orchestrator::live_publication; sim::world::bridge_orchestrator::live_publication::constructor_publication; sim::world::bridge_orchestrator::live_publication::pavement_publication; sim::world::bridge_orchestrator::live_publication::repair_publication; sim::world::bridge_orchestrator::live_publication::repair_publication::occupants; sim::world::bridge_orchestrator::live_publication::rim_publication; sim::world::bridge_orchestrator::live_publication::tile_publication; sim::world::bridge_orchestrator::live_publication::zone_publication; util::fixed_math
sim::world::bridge_orchestrator::live_publication::rim_publication -> map::bridge_facts; map::bridge_rim_tiles; map::cell_index; map::resolved_terrain; rules::ruleset; sim::bridge_state; sim::bridge_state::publication; sim::bridge_state::rim; sim::intern; sim::rng; sim::world; sim::world::bridge_orchestrator; sim::world::bridge_orchestrator::ground_fallout; sim::world::bridge_orchestrator::live_publication; sim::world::bridge_orchestrator::live_publication::constructor_publication; sim::world::bridge_orchestrator::live_publication::pavement_publication; sim::world::bridge_orchestrator::live_publication::repair_publication; sim::world::bridge_orchestrator::live_publication::tile_publication; sim::world::bridge_orchestrator::live_publication::zone_publication; util::fixed_math
sim::world::bridge_orchestrator::live_publication::tile_publication -> map::bridge_facts; map::cell_index; map::iso_tile_flood; map::resolved_terrain; rules::ruleset; sim::bridge_state; sim::bridge_state::publication; sim::intern; sim::rng; sim::world; sim::world::bridge_orchestrator; sim::world::bridge_orchestrator::ground_fallout; sim::world::bridge_orchestrator::live_publication; sim::world::bridge_orchestrator::live_publication::constructor_publication; sim::world::bridge_orchestrator::live_publication::pavement_publication; sim::world::bridge_orchestrator::live_publication::repair_publication; sim::world::bridge_orchestrator::live_publication::rim_publication; sim::world::bridge_orchestrator::live_publication::zone_publication; util::fixed_math
sim::world::bridge_orchestrator::live_publication::zone_publication -> map::bridge_facts; map::cell_index; map::resolved_terrain; rules::ruleset; sim::bridge_state; sim::bridge_state::publication; sim::intern; sim::pathfinding::zone_incremental; sim::rng; sim::world; sim::world::bridge_orchestrator; sim::world::bridge_orchestrator::ground_fallout; sim::world::bridge_orchestrator::live_publication; sim::world::bridge_orchestrator::live_publication::constructor_publication; sim::world::bridge_orchestrator::live_publication::pavement_publication; sim::world::bridge_orchestrator::live_publication::repair_publication; sim::world::bridge_orchestrator::live_publication::rim_publication; sim::world::bridge_orchestrator::live_publication::tile_publication; util::fixed_math
sim::world::building_anim -> rules::art_data; rules::ruleset; sim::intern; sim::production; sim::world
sim::world::command_schedule -> map::entities; rules::locomotor_type; rules::ruleset; sim::combat; sim::command; sim::intern; sim::movement; sim::movement::group_destination; sim::movement::locomotor; sim::pathfinding::core; sim::pathfinding::zone_map; sim::world
sim::world::damage_consequences -> map::entities; rules::overlay_types; rules::ruleset; sim::combat; sim::intern; sim::pathfinding::core; sim::production; sim::world
sim::world::edge_cell -> map::playfield; map::resolved_terrain; sim::cell_rect; sim::pathfinding::core; sim::rng
sim::world::frame_error -> -
sim::world::gap_generator -> map::entities; rules::ruleset; sim::intern; sim::power_system; sim::vision; sim::world
sim::world::hash_schema -> -
sim::world::house_base -> map::actions; map::entities; map::events; map::houses; map::overlay; map::playfield; map::resolved_terrain; map::trigger_graph; map::triggers; rules::locomotor_type; rules::object_type; rules::ruleset; sim::ai; sim::animation; sim::bridge_state; sim::combat; sim::combat::combat_weapon; sim::command; sim::components; sim::docking::aircraft_dock; sim::docking::building_dock; sim::entity_store; sim::find_nearby_cell; sim::house_state; sim::house_strategy; sim::intern; sim::lifecycle_request; sim::movement; sim::movement::drop_pod_movement; sim::movement::locomotor; sim::movement::rocket_movement; sim::movement::teleport_movement; sim::movement::tunnel_movement; sim::movement::turret; sim::occupancy; sim::overlay_grid; sim::passenger; sim::pathfinding::core; sim::pathfinding::terrain_cost; sim::pathfinding::terrain_speed; sim::pathfinding::zone_incremental; sim::pathfinding::zone_map; sim::power_system; sim::production; sim::production::production_types; sim::projectile; sim::radar; sim::rng; sim::scenario_session; sim::team_script_vm; sim::tiberium; sim::trigger_runtime; sim::vision; sim::world; sim::world::authored_load_host; sim::world::bridge_hut_scatter; sim::world::bridge_orchestrator; sim::world::building_anim; sim::world::command_schedule; sim::world::damage_consequences; sim::world::edge_cell; sim::world::frame_error; sim::world::gap_generator; sim::world::hash_schema; sim::world::infantry_terminal; sim::world::jumpjet_cruise; sim::world::lifecycle; sim::world::load_object_lifecycle; sim::world::logic_vector; sim::world::move_cell_input; sim::world::navigation; sim::world::object_turn; sim::world::projectile_collision; sim::world::shroud_refresh; sim::world::substrate; sim::world::techno_ai; sim::world::techno_ai::mission_handlers; sim::world::techno_ai_cloak; sim::world::track_cell_recalc; sim::world::unit_post; sim::world::world_commands; sim::world::world_hash; sim::world::world_orders; sim::world::world_spawn; util::fixed_math; util::native_x87
sim::world::infantry_terminal -> map::entities; rules::animation_sequence; rules::ruleset; sim::animation; sim::combat; sim::components; sim::world
sim::world::jumpjet_cruise -> map::cell_index; map::entities; map::resolved_terrain; map::retail_trig; rules::locomotor_type; rules::ruleset; sim::components; sim::entity_store; sim::game_entity; sim::intern; sim::movement::air_movement; sim::movement::ground_pose; sim::movement::jumpjet_flight; sim::movement::jumpjet_movement; sim::movement::locomotor; sim::occupancy; sim::rng; sim::world; util::fixed_math; util::lepton
sim::world::lifecycle -> map::entities; rules::ruleset; sim::cell_rect; sim::combat; sim::components; sim::game_entity; sim::intern; sim::lifecycle_request; sim::movement::locomotor; sim::occupancy; sim::passenger; sim::projectile; sim::world; sim::world::substrate; util::fixed_math; util::lepton
sim::world::load_object_lifecycle -> -
sim::world::logic_vector -> -
sim::world::move_cell_input -> map::cell_index; map::playfield; map::resolved_terrain; rules::locomotor_type; sim::cell_rect; sim::components; sim::find_nearby_cell; sim::movement::ground_pose; sim::occupancy; sim::pathfinding::zone_map; sim::world
sim::world::navigation -> map::entities; map::resolved_terrain; rules::locomotor_type; rules::ruleset; sim::bridge_state; sim::entity_store; sim::intern; sim::pathfinding::core; sim::pathfinding::terrain_cost; sim::pathfinding::zone_map
sim::world::object_turn -> map::entities; rules::ruleset; sim::lifecycle_request; sim::movement; sim::movement::homing_movement; sim::movement::parachute_descent; sim::movement::rocket_movement; sim::movement::teleport_movement; sim::pathfinding::core; sim::world; sim::world::techno_ai
sim::world::projectile_collision -> map::actions; map::entities; map::events; map::houses; map::overlay; map::playfield; map::resolved_terrain; map::trigger_graph; map::triggers; rules::locomotor_type; rules::object_type; rules::ruleset; sim::ai; sim::animation; sim::bridge_state; sim::cell_rect; sim::combat; sim::combat::combat_weapon; sim::command; sim::components; sim::docking::aircraft_dock; sim::docking::building_dock; sim::entity_store; sim::game_entity; sim::house_state; sim::house_strategy; sim::intern; sim::lifecycle_request; sim::movement; sim::movement::drop_pod_movement; sim::movement::locomotor; sim::movement::rocket_movement; sim::movement::teleport_movement; sim::movement::tunnel_movement; sim::movement::turret; sim::occupancy; sim::overlay_grid; sim::passenger; sim::pathfinding::core; sim::pathfinding::terrain_cost; sim::pathfinding::terrain_speed; sim::pathfinding::zone_incremental; sim::pathfinding::zone_map; sim::power_system; sim::production; sim::production::production_types; sim::projectile; sim::radar; sim::rng; sim::scenario_session; sim::team_script_vm; sim::tiberium; sim::trigger_runtime; sim::vision; sim::world; sim::world::authored_load_host; sim::world::bridge_hut_scatter; sim::world::bridge_orchestrator; sim::world::building_anim; sim::world::command_schedule; sim::world::damage_consequences; sim::world::edge_cell; sim::world::frame_error; sim::world::gap_generator; sim::world::hash_schema; sim::world::house_base; sim::world::infantry_terminal; sim::world::jumpjet_cruise; sim::world::lifecycle; sim::world::load_object_lifecycle; sim::world::logic_vector; sim::world::move_cell_input; sim::world::navigation; sim::world::object_turn; sim::world::shroud_refresh; sim::world::substrate; sim::world::techno_ai; sim::world::techno_ai::mission_handlers; sim::world::techno_ai_cloak; sim::world::track_cell_recalc; sim::world::unit_post; sim::world::world_commands; sim::world::world_hash; sim::world::world_orders; sim::world::world_spawn; util::fixed_math; util::native_x87
sim::world::shroud_refresh -> map::entities; rules::locomotor_type; rules::ruleset; sim::game_entity; sim::pathfinding::core; sim::vision; sim::world
sim::world::substrate -> sim::anim_class; sim::cell_rect; sim::entity_store; sim::occupancy; sim::particles; sim::voxel_anim; sim::world::logic_vector
sim::world::techno_ai -> map::entities; rules::mission_data; rules::overlay_types; rules::particle_system_type; rules::ruleset; sim::miner; sim::pathfinding::core; sim::world; sim::world::techno_ai::mission_handlers
sim::world::techno_ai::bounce_terrain -> map::entities; rules::ruleset; sim::bounce; sim::cell_rect; sim::movement::locomotor; sim::world
sim::world::techno_ai::mission_handlers -> map::entities; rules::mission_data; rules::ruleset; rules::terrain_rules; sim::combat::threat_range; sim::components; sim::game_entity; sim::mission::authority; sim::mission::state; sim::world; sim::world::techno_ai; util::native_x87
sim::world::techno_ai_cloak -> map::entities; rules::ruleset; sim::combat; sim::components; sim::intern; sim::mission::concrete_effects; sim::movement::locomotor; sim::world
sim::world::track_cell_recalc -> rules::overlay_types; rules::ruleset; sim::world; sim::world::navigation
sim::world::unit_post -> rules::ruleset; sim::combat; sim::entity_store; sim::intern
sim::world::world_commands -> map::cell_index; map::houses; rules::locomotor_type; rules::mission_data; rules::object_type; rules::ruleset; sim::combat; sim::combat::combat_aoe; sim::command; sim::components; sim::docking::building_dock; sim::mission::retask; sim::movement; sim::movement::bump_crush; sim::movement::jumpjet_movement; sim::movement::locomotor; sim::movement::teleport_movement; sim::overlay_grid; sim::passenger; sim::pathfinding::core; sim::pathfinding::terrain_cost; sim::pathfinding::zone_incremental; sim::production; sim::world; util::fixed_math
sim::world::world_hash -> sim::game_entity; sim::house_state; sim::mission::leaf; sim::mission::state; sim::movement::locomotion::piggyback; sim::movement::slope_transition; sim::projectile; sim::world; sim::world::hash_schema
sim::world::world_orders -> map::entities; rules::mission_data; rules::ruleset; sim::combat; sim::components; sim::intern; sim::movement; sim::movement::bump_crush; sim::movement::locomotor; sim::pathfinding::core; sim::world; util::fixed_math
sim::world::world_spawn -> map::entities; map::resolved_terrain; rules::object_type; rules::ruleset; sim::base_plan; sim::base_plan_generation; sim::components; sim::game_entity; sim::intern; sim::movement::locomotor; sim::production::production_tech; sim::production::production_types; sim::world; sim::world::lifecycle
sim::world::world_spawn::authored_health -> map::entities; map::resolved_terrain; rules::object_type; rules::ruleset; sim::base_plan; sim::base_plan_generation; sim::components; sim::game_entity; sim::intern; sim::movement::locomotor; sim::production::production_tech; sim::production::production_types; sim::world; sim::world::lifecycle; sim::world::world_spawn; sim::world::world_spawn::construction; util::native_x87
sim::world::world_spawn::construction -> map::entities; rules::animation_sequence; rules::locomotor_type; rules::object_type; rules::ruleset; sim::animation; sim::components; sim::game_entity; sim::miner; sim::movement::locomotor; sim::world; sim::world::world_spawn
skirmish_cooperative -> assets::asset_manager; rules::ini_parser; sim::rng
skirmish_launch -> sim::game_options; sim::rng; skirmish_modes
skirmish_modes -> assets::asset_manager; rules::ini_parser
skirmish_persistence -> rules::error
ui -> -
ui::client_theme -> -
ui::gadget -> -
ui::gadget::button -> ui::gadget::focus; ui::gadget::list
ui::gadget::focus -> ui::gadget
ui::gadget::list -> ui::gadget; ui::gadget::focus
ui::gadget::tick -> ui::gadget; ui::gadget::button; ui::gadget::focus; ui::gadget::list
ui::game_screen -> -
ui::main_menu -> map::scenario_menu; ui::client_theme
ui::main_menu_dialogs -> ui::client_theme; ui::main_menu_dialogs::options
ui::main_menu_dialogs::options -> ui::client_theme; ui::main_menu_dialogs::options::shell
ui::main_menu_dialogs::options::shell -> ui::main_menu_dialogs::options; ui::shell::geom; ui::skirmish_shell::scroll
ui::main_menu_shell -> ui::main_menu_shell::layout; ui::main_menu_shell::state; ui::shell::geom
ui::main_menu_shell::layout -> ui::main_menu_shell::state; ui::shell::descriptor; ui::shell::geom; ui::shell::layout
ui::main_menu_shell::state -> ui::shell::static_reveal
ui::messages -> -
ui::mission_status -> ui::client_theme
ui::pause_menu -> ui::client_theme; ui::skirmish_shell::layout
ui::score_shell -> ui::shell::geom
ui::shell -> -
ui::shell::abort -> ui::shell::geom; ui::shell::in_game_shell
ui::shell::button -> -
ui::shell::controller -> ui::shell::descriptor; ui::shell::layout
ui::shell::descriptor -> ui::shell::geom
ui::shell::geom -> -
ui::shell::in_game_options -> ui::shell::descriptor; ui::shell::geom
ui::shell::in_game_options::control -> -
ui::shell::in_game_options_state -> ui::main_menu_dialogs::options; ui::shell::button; ui::shell::geom
ui::shell::in_game_shell -> ui::shell::geom
ui::shell::keyboard -> ui::shell::button; ui::shell::geom; ui::shell::in_game_shell; ui::shell::list; ui::shell::static_reveal
ui::shell::layout -> ui::shell::descriptor; ui::shell::geom
ui::shell::list -> ui::shell::geom
ui::shell::modal -> ui::shell::descriptor; ui::shell::geom
ui::shell::modal::control -> -
ui::shell::pause_menu -> ui::shell::button; ui::shell::geom; ui::shell::in_game_shell
ui::shell::saved_file_input -> ui::shell::list; ui::skirmish_shell::layout; ui::skirmish_shell::state::saved_seed_browser
ui::shell::saved_games -> ui::shell::geom; ui::shell::in_game_shell; ui::skirmish_shell::layout
ui::shell::slide -> ui::shell::descriptor
ui::shell::sound -> ui::main_menu_dialogs::options; ui::shell::button; ui::shell::geom; ui::shell::in_game_shell; ui::shell::list
ui::shell::static_reveal -> -
ui::single_player_shell -> ui::single_player_shell::layout; ui::single_player_shell::state
ui::single_player_shell::layout -> ui::main_menu_shell::layout; ui::shell::geom; ui::single_player_shell::state
ui::single_player_shell::state -> -
ui::skirmish_shell -> ui::shell::geom; ui::skirmish_shell::layout; ui::skirmish_shell::scroll; ui::skirmish_shell::state; ui::skirmish_shell::state::choose_map; ui::skirmish_shell::state::combos; ui::skirmish_shell::state::hit_test; ui::skirmish_shell::state::launch; ui::skirmish_shell::state::player_name; ui::skirmish_shell::state::random_map_setup; ui::skirmish_shell::state::saved_seed_browser; ui::skirmish_shell::state::trackbars
ui::skirmish_shell::layout -> ui::shell::geom; ui::skirmish_shell::scroll
ui::skirmish_shell::scroll -> ui::shell::geom
ui::skirmish_shell::seed_list -> ui::shell::list; ui::skirmish_shell::layout
ui::skirmish_shell::state -> skirmish_launch; ui::main_menu; ui::skirmish_shell::layout; ui::skirmish_shell::state::choose_map; ui::skirmish_shell::state::combos; ui::skirmish_shell::state::hit_test; ui::skirmish_shell::state::launch; ui::skirmish_shell::state::player_name; ui::skirmish_shell::state::random_map_setup; ui::skirmish_shell::state::saved_seed_browser; ui::skirmish_shell::state::trackbars
ui::skirmish_shell::state::choose_map -> map::skirmish_scenarios; skirmish_modes; ui::shell::geom; ui::skirmish_shell::layout
ui::skirmish_shell::state::combos -> map::scenario_menu; ui::main_menu; ui::shell::geom; ui::skirmish_shell::layout; ui::skirmish_shell::scroll; ui::skirmish_shell::state; ui::skirmish_shell::state::player_name
ui::skirmish_shell::state::hit_test -> map::scenario_menu; ui::shell::geom; ui::skirmish_shell::layout; ui::skirmish_shell::state; ui::skirmish_shell::state::choose_map; ui::skirmish_shell::state::combos; ui::skirmish_shell::state::player_name; ui::skirmish_shell::state::trackbars
ui::skirmish_shell::state::launch -> map::scenario_menu; skirmish_launch; skirmish_modes; ui::main_menu; ui::skirmish_shell::state::player_name
ui::skirmish_shell::state::player_name -> map::scenario_menu; sim::game_options; skirmish_launch; skirmish_modes; ui::main_menu; ui::skirmish_shell::layout; ui::skirmish_shell::state; ui::skirmish_shell::state::choose_map; ui::skirmish_shell::state::random_map_setup; ui::skirmish_shell::state::trackbars; ui::skirmish_shell::static_reveal
ui::skirmish_shell::state::random_map_setup -> map::rmg::options; map::rmg::preview; map::rmg::randomize; map::rmg::settings; ui::skirmish_shell::layout; ui::skirmish_shell::state::choose_map
ui::skirmish_shell::state::saved_seed_browser -> map::rmg::description; map::rmg::saved_seeds; ui::shell::geom; ui::skirmish_shell::layout; util::native_file_name
ui::skirmish_shell::state::trackbars -> map::scenario_menu; rules::ini_parser; ui::shell::geom; ui::skirmish_shell::layout; ui::skirmish_shell::state; ui::skirmish_shell::state::combos; ui::skirmish_shell::state::player_name
ui::skirmish_shell::static_reveal -> -
ui::tooltips -> -
util -> -
util::base64 -> -
util::config -> -
util::direction -> -
util::direction_tables -> util::direction_tables::cell; util::direction_tables::dragon; util::direction_tables::lepton; util::direction_tables::native_angle; util::direction_tables::quantize
util::direction_tables::cell -> -
util::direction_tables::dragon -> -
util::direction_tables::lepton -> -
util::direction_tables::native_angle -> util::native_x87
util::direction_tables::native_angle_table -> -
util::direction_tables::quantize -> -
util::facing_table -> util::fixed_math
util::fixed_math -> -
util::flh_transform -> -
util::fnv -> -
util::ini_writer -> -
util::lcw -> -
util::legacy_crt_rng -> -
util::lepton -> util::fixed_math
util::logging -> -
util::lzo -> -
util::native_file_name -> -
util::native_file_time -> -
util::native_string -> -
util::native_trig -> -
util::native_x87 -> util::native_x87::masked
util::native_x87::masked -> util::native_x87
util::pixel_conversion -> -
util::read_helpers -> -
util::retail_pointer_sort -> -
util::sha256 -> -
util::single_instance -> -
util::version -> -
vera20k -> -
```
<!-- module-map:dependencies:end -->
