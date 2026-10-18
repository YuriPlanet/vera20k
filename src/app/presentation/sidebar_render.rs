//! Sidebar view construction, minimap interaction, chrome helpers, and render pass.
//!
//! Refreshes the retained SidebarView projection at explicit state transitions,
//! handles minimap drag/click, resolves sidebar chrome theme, and creates the
//! main wgpu render pass.
//!
//! Instance builders for sidebar layers live in `presentation::sidebar_build`.
//!
//! Split from the presentation render path to keep files under 400 lines.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::app::input::commands::preferred_local_owner_name;
use crate::sim::production;
use crate::ui::sidebar::{self, SidebarView};

// Re-export instance builders so callers don't need to know about the split.
pub(crate) use crate::app::presentation::sidebar_build::{
    build_command_bar_instances, build_sidebar_cameo_instances, build_sidebar_chrome_instances,
    build_sidebar_text_instances,
};

// ---------------------------------------------------------------------------
// Sidebar view construction
// ---------------------------------------------------------------------------

/// Return the one retained sidebar projection. Reading it never advances
/// credits, clears targeting, or clamps scroll state.
pub(crate) fn current_sidebar_view(state: &AppState) -> Option<&SidebarView> {
    state
        .match_state
        .match_presentation
        .sidebar_projection
        .view()
}

/// The money the credits counter counts toward: `CreditsClass::AI @
/// 0x004A2600` reads the player's Available_Money and shows a balance at or
/// below zero as 0 (`0x004A2687..0x004A2698`).
pub(crate) fn counter_credits(sim: &crate::sim::world::Simulation, owner: &str) -> i32 {
    production::credits_for_owner(sim, owner).max(0)
}

/// Advance the displayed balance at the authoritative gameplay-frame seam.
pub(crate) fn advance_sidebar_credits_after_frame(
    state: &mut AppState,
    frame_committed: bool,
    tick_lane: crate::sim::world::TickLane,
) {
    if !crate::app::sidebar_projection::credits_advance_for_frame(frame_committed, tick_lane) {
        return;
    }
    let owner_name = preferred_local_owner_name(state).unwrap_or_else(|| "Americans".to_string());
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return;
    };
    let credits = counter_credits(sim, &owner_name);
    let Some(tick) = state
        .match_state
        .match_presentation
        .sidebar_projection
        .advance_credits(&owner_name, credits)
    else {
        return;
    };
    // `CreditsClass::Draw @ 0x004A24F4..0x004A2533`: one `CreditTicks` cue per
    // changed AI step — `[0]` counting up, `[1]` counting down — at volume
    // `0.5f`, centre pan, for the local player's counter only (the observer
    // branch draws elapsed time instead). Native plays it from the next
    // sidebar draw, which clears the latch; VERA emits at the step itself.
    let Some(sound_id) = state.rules().and_then(|rules| {
        crate::app::sidebar_projection::credit_tick_sound(&rules.general.credit_ticks, tick)
            .map(str::to_string)
    }) else {
        return;
    };
    state
        .match_state
        .match_audio
        .sound_events
        .push(crate::audio::events::GameSoundEvent::CreditTick { sound_id });
}

/// Apply one reconcile pass's native tab transitions.
///
/// AddCameo 6A6472 enables the button of the strip that just accepted an
/// entry; Recalculate 6AA6xx disables the button of the strip it emptied, and
/// both then hand the active tab to the first populated strip when that is not
/// already the active one (the same scan is used by the trigger action's
/// `StripClass::HasEntries` gate, see `SidebarProjectionState::selectable_tab`).
/// No strip populated leaves the active tab alone.
fn apply_cameo_transitions(
    transitions: crate::ui::sidebar::cameo_order::CameoTransitions,
    active_tab: &mut sidebar::SidebarTab,
    gadgets: &mut crate::ui::sidebar::gadget_flash::SidebarGadgetState,
) {
    for tab in sidebar::SidebarTab::all() {
        if let Some(disabled) = transitions.tab_button_disabled(tab) {
            gadgets.set_tab_disabled(tab.tab_index(), disabled);
        }
    }
    if let Some(destination) = transitions.active_tab_destination() {
        *active_tab = destination;
    }
}

/// Reconcile state-derived sidebar inputs and replace the retained immutable
/// projection. This is called only from explicit simulation/input/lifecycle
/// transitions, never from a view consumer.
pub(crate) fn refresh_sidebar_projection(state: &mut AppState) {
    refresh_radar_animation_source(state);
    let mut spec =
        crate::ui::sidebar::SidebarChromeLayoutSpec::for_theme(current_sidebar_theme(state));
    if let Some(atlas) = current_sidebar_chrome(state) {
        spec.side2_height = atlas.side2.pixel_size[1];
        spec.side3_height = atlas.side3.pixel_size[1];
    }
    state.match_state.match_presentation.sidebar_layout_spec = spec;
    // The sidebar lays out on its own surface (`ui::sidebar::SidebarFrame`).
    let (screen_w, screen_h) = state.sidebar_frame().layout_screen();
    let visible_rows =
        sidebar::compute_layout_with_spec(spec, screen_w, screen_h, 0).side2_tile_count;
    let owner_name: String =
        preferred_local_owner_name(state).unwrap_or_else(|| "Americans".to_string());
    let Some((
        build_options,
        queue_items,
        ready_buildings,
        producer_focus,
        credits,
        power_produced,
        power_drained,
        sw_views,
    )) = (|| {
        let (sim, rules) = (
            state
                .match_state
                .sim_runtime
                .as_ref()
                .map(|rt| &rt.simulation)?,
            state.rules()?,
        );
        let producer_focus = [
            production::ProductionCategory::Building,
            production::ProductionCategory::Defense,
            production::ProductionCategory::Infantry,
            production::ProductionCategory::Vehicle,
            production::ProductionCategory::Aircraft,
            production::ProductionCategory::Ship,
        ]
        .into_iter()
        .filter_map(|category| {
            production::active_producer_for_owner_category(sim, rules, &owner_name, category)
        })
        .collect::<Vec<_>>();
        let owner_iid = sim.interner.get(&owner_name).unwrap_or_default();
        let sw_views = crate::sim::superweapon::superweapon_views_for_owner(sim, rules, &owner_iid);
        let (power_produced, power_drained) =
            production::power_balance_for_owner(sim, rules, &owner_name);
        Some((
            production::all_build_options_for_owner(sim, rules, &owner_name),
            production::queue_view_for_owner(sim, rules, &owner_name),
            production::ready_buildings_for_owner(sim, rules, &owner_name),
            producer_focus,
            counter_credits(sim, &owner_name),
            power_produced,
            power_drained,
            sw_views,
        ))
    })()
    else {
        state
            .match_state
            .match_presentation
            .sidebar_projection
            .replace_view(None);
        return;
    };

    let sim = &state
        .match_state
        .sim_runtime
        .as_ref()
        .expect("sidebar runtime")
        .simulation;
    let owner = sim.interner.get(&owner_name).unwrap_or_default();
    let seed_additions = state
        .match_state
        .match_presentation
        .sidebar_projection
        .needs_cameo_seed(owner)
        .then(|| {
            production::initial_construction_options(sim, state.rules().expect("sidebar rules"))
        })
        .unwrap_or_default();
    let cameos = crate::app::sidebar_projection::cameo_candidates(
        sim,
        state.rules().expect("sidebar rules"),
        state.process_assets.csf.as_ref(),
        owner,
        &build_options,
        &ready_buildings,
        &sw_views,
    );
    let reconciled = state
        .match_state
        .match_presentation
        .sidebar_projection
        .reconcile_cameos(owner, &cameos, visible_rows, &seed_additions);
    state
        .match_state
        .match_presentation
        .sidebar_projection
        .set_cameo_frame(sim.session.binary_frame as i32);
    state
        .match_state
        .match_presentation
        .sidebar_projection
        .reconcile_factory_links(sim, owner);
    state
        .match_state
        .match_presentation
        .sidebar_projection
        .advance_cameo_requests(sim, owner);
    {
        let presentation = &mut state.match_state.match_presentation;
        apply_cameo_transitions(
            reconciled.transitions(),
            &mut presentation.active_sidebar_tab,
            &mut presentation.sidebar_gadget_state,
        );
    }
    if reconciled.build_inserted() {
        crate::app::input::dispatch::push_local_eva(
            state,
            crate::app::input::sidebar_eva::EVA_NEW_CONSTRUCTION_OPTIONS,
        );
    }

    let display_credits = state
        .match_state
        .match_presentation
        .sidebar_projection
        .displayed_credits_or_seed(&owner_name, credits);
    let (
        tab_btn_size,
        repair_btn_size,
        sell_btn_size,
        scroll_down_btn_size,
        scroll_up_btn_size,
        top_btn_sizes,
    ) = {
        let scale = state.match_state.match_presentation.ui_scale;
        let size = |entry: Option<&crate::render::sidebar_chrome::SidebarChromeEntry>| {
            entry.map(|entry| [entry.pixel_size[0] * scale, entry.pixel_size[1] * scale])
        };
        let atlas = current_sidebar_chrome(state);
        (
            size(atlas.and_then(|atlas| atlas.tab_frames[0][0].as_ref())),
            size(atlas.and_then(|atlas| atlas.repair_frames[0].as_ref())),
            size(atlas.and_then(|atlas| atlas.sell_frames[0].as_ref())),
            size(atlas.and_then(|atlas| atlas.scroll_down_frames[0].as_ref())),
            size(atlas.and_then(|atlas| atlas.scroll_up_frames[0].as_ref())),
            std::array::from_fn(|i| {
                size(atlas.and_then(|atlas| atlas.top_button_frames[i][0].as_ref()))
            }),
        )
    };
    // Placement lifetime is driven by the input owner's ordered native
    // lifecycle facts. A missing Ready row alone is not pointer expiry:
    // Dispatch7258D0 can clear product while retaining type/foundation.
    // App targeting state -> the sidebar-owned armed projection (F06 seam).
    let armed_entry = state
        .match_state
        .input
        .targeting_mode
        .as_ref()
        .map(|mode| match mode {
            crate::app::types::TargetingMode::BuildingPlacement(section) => {
                sidebar::ArmedSidebarEntry::BuildingPlacement(section.type_id().to_string())
            }
            crate::app::types::TargetingMode::SuperWeapon(section) => {
                sidebar::ArmedSidebarEntry::SuperWeapon(section.clone())
            }
        });
    let factory_statuses = state
        .match_state
        .match_presentation
        .sidebar_projection
        .factory_statuses(
            &state
                .match_state
                .sim_runtime
                .as_ref()
                .expect("sidebar runtime")
                .simulation,
            owner,
        );
    let mut view = sidebar::build_sidebar_view_with_spec(
        state.match_state.match_presentation.sidebar_layout_spec,
        screen_w,
        screen_h,
        state.match_state.match_presentation.active_sidebar_tab,
        display_credits,
        power_produced,
        power_drained,
        tab_btn_size,
        &queue_items,
        &factory_statuses,
        &build_options,
        &ready_buildings,
        armed_entry.as_ref(),
        &producer_focus,
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)
            .map(|sim| &sim.interner),
        &sw_views,
        &state.match_state.match_presentation.sidebar_gadget_state,
        repair_btn_size,
        sell_btn_size,
        scroll_down_btn_size,
        scroll_up_btn_size,
        top_btn_sizes,
        state
            .match_state
            .match_presentation
            .sidebar_projection
            .cameo_strips(),
    );
    // The view never substitutes another tab, so the retained active tab needs
    // no write-back: `apply_cameo_transitions` (and the switch itself) is the
    // only writer.
    state
        .match_state
        .match_presentation
        .sidebar_projection
        .set_scroll_row(
            state.match_state.match_presentation.active_sidebar_tab,
            view.scroll_rows,
        );
    if let Some(atlas) = state
        .match_state
        .match_presentation
        .sidebar_cameo_atlas
        .as_ref()
    {
        let house = state
            .match_state
            .sim_runtime
            .as_ref()
            .and_then(|rt| rt.simulation.houses.get(&owner));
        let rules = state.rules();
        for item in &mut view.items {
            item.alt_cameo = house.is_some_and(|house| {
                rules
                    .and_then(|rules| rules.object(&item.type_id))
                    .is_some_and(|obj| shows_alt_cameo(house, obj))
            });
            item.has_cameo_art = atlas.cameo(&item.type_id, item.alt_cameo).is_some();
        }
    }
    state
        .match_state
        .match_presentation
        .sidebar_projection
        .replace_view(Some(view));
    // A rebuilt view starts from the update-time answers; keep the drawn ones
    // authoritative from its first frame on.
    refresh_sidebar_cameo_availability(state);
}

/// The per-frame part of the projection: the retained cameos' `StripDraw`
/// unavailability inputs, taken from the live simulation. `StripClass::Draw @
/// 0x006A9540` asks `FindFactory(1, 1, 1)`, `CanBuild(type, 0, 0)` and
/// `CheckBuildLimit` for every entry it draws, so a restriction that appears or
/// clears between two House rechecks must reach the very next frame. Membership
/// and the House pass stay with [`refresh_sidebar_projection`].
pub(crate) fn refresh_sidebar_cameo_availability(state: &mut AppState) {
    // Without a current House the projection's owner comes from the sandbox
    // guess, whose resolution scans every entity and each candidate's build
    // options. That debug path keeps its update-time values.
    if state
        .match_state
        .sim_runtime
        .as_ref()
        .is_none_or(|runtime| runtime.simulation.session.current_house.is_none())
    {
        return;
    }
    let owner_name = preferred_local_owner_name(state).unwrap_or_else(|| "Americans".to_string());
    let AppState { match_state, .. } = state;
    let Some(runtime) = match_state.sim_runtime.as_ref() else {
        return;
    };
    let owner = runtime
        .simulation
        .interner
        .get(&owner_name)
        .unwrap_or_default();
    match_state
        .match_presentation
        .sidebar_projection
        .refresh_cameo_availability(&runtime.simulation, &runtime.resources.rules, owner);
}

// ---------------------------------------------------------------------------
// Minimap interaction
// ---------------------------------------------------------------------------

/// `TechnoTypeClass::GetCameo @ 0x00712040`'s choice of the `AltCameo=`
/// image: a `Trainable=` type whose veterancy the local player's spies took
/// (`HouseClass+0x2BF` for infantry; `+0x2C0` for vehicles, ships included,
/// and for buildings that undeploy). [`SidebarCameoAtlas::cameo`] keeps the
/// plain cameo when no alternate image loaded.
///
/// [`SidebarCameoAtlas::cameo`]: crate::render::sidebar_cameo_atlas::SidebarCameoAtlas::cameo
fn shows_alt_cameo(
    house: &crate::sim::house_state::HouseState,
    obj: &crate::rules::object_type::ObjectType,
) -> bool {
    use crate::rules::object_type::ObjectCategory;
    obj.trainable
        && match obj.category {
            ObjectCategory::Infantry => house.veteran_infantry(),
            ObjectCategory::Vehicle => house.veteran_units(),
            ObjectCategory::Building => house.veteran_units() && obj.undeploys_into.is_some(),
            ObjectCategory::Aircraft => false,
        }
}

pub(crate) fn is_cursor_over_minimap(state: &AppState) -> bool {
    // Minimap interaction disabled when radar is not online.
    let minimap_visible: bool = state
        .match_state
        .match_presentation
        .radar_anim
        .as_ref()
        .is_none_or(|ra| ra.is_minimap_visible());
    if !minimap_visible {
        return false;
    }
    let Some(minimap) = &state.match_state.match_presentation.minimap else {
        return false;
    };
    let rect = active_minimap_screen_rect(state);
    let (x, y) = sidebar_cursor(state);
    minimap.contains_screen_point_in_rect(x, y, rect.x, rect.y, rect.w, rect.h)
}

pub(crate) fn try_begin_minimap_drag(state: &mut AppState) -> bool {
    if !is_cursor_over_minimap(state) {
        return false;
    }
    // RadarClass input @ 0x006539D0 routes ordinary selected objects to the
    // camera branch before DisplayClass::BandBox_LeftUp. A minimap press never
    // gains selected-unit Move/AttackMove precedence.
    state.match_state.input.minimap_dragging = true;
    state.match_state.input.selection_state.cancel_drag();
    update_camera_from_minimap_cursor(state);
    true
}

pub(crate) fn minimap_camera_target(
    state: &AppState,
) -> Option<crate::render::radar::NativeRadarCameraTarget> {
    let rect = active_minimap_screen_rect(state);
    let (cursor_x, cursor_y) = sidebar_cursor(state);
    let minimap = state.match_state.match_presentation.minimap.as_ref()?;
    let runtime = state.match_state.sim_runtime.as_ref();
    let native_facts = runtime.and_then(|runtime| {
        let view = runtime.view();
        Some((
            view.simulation(),
            &runtime.resources.rules,
            view.resolved_terrain()?,
            view.playfield_map_size()?,
        ))
    });
    let (tactical_w, tactical_h) = crate::app::input::camera::tactical_viewport_size(state);
    native_facts.and_then(|(simulation, rules, terrain, map_size)| {
        minimap.native_click_target_in_rect(
            cursor_x,
            cursor_y,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            simulation,
            rules,
            terrain,
            map_size,
            (tactical_w as i32, tactical_h as i32),
        )
    })
}

pub(crate) fn minimap_object_click_position(state: &AppState, id: u64) -> Option<[u32; 2]> {
    let rect = active_minimap_screen_rect(state);
    let minimap = state.match_state.match_presentation.minimap.as_ref()?;
    let (x, y) = minimap.native_object_click_point_in_rect(id, [rect.x, rect.y, rect.w, rect.h])?;
    let frame = state
        .sidebar_frame()
        .rect_to_frame(crate::ui::sidebar::Rect {
            x,
            y,
            w: 1.0,
            h: 1.0,
        });
    Some([frame.x.round() as u32, frame.y.round() as u32])
}

pub(crate) fn update_camera_from_minimap_cursor(state: &mut AppState) {
    let native_target = minimap_camera_target(state);
    if let Some(target) = native_target {
        // `0x00653EA0 -> FUN_006D6070` writes current and desired viewport
        // together from the complete CellClass center XYZ. The camera owner
        // discards any pending scroll request when setting the absolute view.
        let (x, y, z) = target.world_leptons;
        crate::app::input::camera::center_camera_on_lepton_point(state, x, y, z);
        return;
    }
    let rect = active_minimap_screen_rect(state);
    let Some(minimap) = state.match_state.match_presentation.minimap.as_ref() else {
        return;
    };
    if minimap.has_playfield_authority() {
        // A live map whose generated surface is unavailable must not silently
        // approximate the click through the old normalized 200x200 transform.
        return;
    }

    // Mapless/headless presentation fixture adapter only.
    let (cursor_x, cursor_y) = sidebar_cursor(state);
    let sw = state.render_width() as f32;
    let sh = state.render_height() as f32;
    let (tactical_w, tactical_h) = crate::app::input::camera::tactical_viewport_size(state);
    let z = state.match_state.input.zoom_level;
    let (cx, cy) = minimap.camera_top_left_for_screen_point_in_rect(
        cursor_x,
        cursor_y,
        tactical_w as f32 / z,
        tactical_h as f32 / z,
        rect.x,
        rect.y,
        rect.w,
        rect.h,
    );
    crate::app::input::camera::set_camera_position(&mut state.match_state.input, (cx, cy));
    crate::app::input::camera::clamp_camera_to_playable_area(state, sw, sh);
}

/// Live generated-primary screen rectangle used by the retained input region.
/// Centered letterbox margins are not part of RadarClass's click surface.
pub(crate) fn active_minimap_content_screen_rect(state: &AppState) -> crate::ui::sidebar::Rect {
    let aperture = active_minimap_screen_rect(state);
    let Some(minimap) = state.match_state.match_presentation.minimap.as_ref() else {
        return crate::ui::sidebar::Rect {
            x: aperture.x,
            y: aperture.y,
            w: 0.0,
            h: 0.0,
        };
    };
    let Some([x, y, w, h]) =
        minimap.content_screen_rect_in_rect(aperture.x, aperture.y, aperture.w, aperture.h)
    else {
        return crate::ui::sidebar::Rect {
            x: aperture.x,
            y: aperture.y,
            w: 0.0,
            h: 0.0,
        };
    };
    crate::ui::sidebar::Rect { x, y, w, h }
}

/// The cursor in the sidebar's layout space, where the minimap rects are.
fn sidebar_cursor(state: &AppState) -> (f32, f32) {
    state.sidebar_frame().point_to_layout(
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    )
}

/// The radar aperture in the sidebar's layout space. Input regions take it in
/// frame pixels through [`crate::ui::sidebar::SidebarFrame::rect_to_frame`].
pub(crate) fn active_minimap_screen_rect(state: &AppState) -> crate::ui::sidebar::Rect {
    let (sw, sh) = state.sidebar_frame().layout_screen();
    if current_sidebar_chrome(state).is_some() {
        crate::ui::sidebar::radar_minimap_rect_with_spec(
            sw,
            state.match_state.match_presentation.sidebar_layout_spec,
        )
    } else {
        let (x, y, w, h) = crate::render::radar::minimap::default_minimap_rect(sh);
        crate::ui::sidebar::Rect { x, y, w, h }
    }
}

// ---------------------------------------------------------------------------
// Chrome / theme helpers
// ---------------------------------------------------------------------------

pub(crate) fn current_sidebar_theme(
    state: &AppState,
) -> crate::render::sidebar_chrome::SidebarTheme {
    preferred_local_owner_name(state)
        .and_then(|owner| {
            sidebar_theme_for_owner(
                state
                    .match_state
                    .sim_runtime
                    .as_ref()
                    .map(|rt| &rt.simulation),
                &owner,
            )
        })
        .unwrap_or(crate::render::sidebar_chrome::SidebarTheme::Allied)
}

/// Shared installed-owner and atlas fallback projection for chrome and radar.
/// The map loader calls the final sidebar refresh only after installing the
/// new simulation and pinned owner; this helper never reads old load inputs.
pub(crate) fn project_sidebar_source<'a, T>(
    simulation: Option<&crate::sim::world::Simulation>,
    owner: Option<&str>,
    allied: Option<&'a T>,
    soviet: Option<&'a T>,
    yuri: Option<&'a T>,
) -> Option<(
    crate::render::sidebar_chrome::SidebarTheme,
    crate::render::sidebar_chrome::SidebarTheme,
    &'a T,
)> {
    let requested = owner
        .and_then(|owner| sidebar_theme_for_owner(simulation, owner))
        .unwrap_or(crate::render::sidebar_chrome::SidebarTheme::Allied);
    let (actual, source) =
        crate::render::sidebar_chrome::select_sidebar_theme(requested, allied, soviet, yuri)?;
    Some((requested, actual, source))
}

fn current_sidebar_resolution(
    state: &AppState,
) -> Option<crate::render::sidebar_chrome::ResolvedSidebarChrome<'_>> {
    let set = state
        .match_state
        .match_presentation
        .sidebar_chrome
        .as_ref()?;
    let owner = preferred_local_owner_name(state);
    let (requested_theme, actual_theme, atlas) = project_sidebar_source(
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        owner.as_deref(),
        set.allied.as_ref(),
        set.soviet.as_ref(),
        set.yuri.as_ref(),
    )?;
    Some(crate::render::sidebar_chrome::ResolvedSidebarChrome {
        requested_theme,
        actual_theme,
        atlas,
    })
}

pub(crate) fn current_sidebar_chrome(
    state: &AppState,
) -> Option<&crate::render::sidebar_chrome::SidebarChromeAtlas> {
    current_sidebar_resolution(state).map(|resolved| resolved.atlas)
}

/// 652E90 installs radar shape/palette from the current scenario side, without
/// resetting frame/time. VERA's unpinned sandbox owner switch uses a forced
/// source redraw; this development interaction has no native lifecycle claim.
fn refresh_radar_animation_source(state: &mut AppState) {
    let Some(resolved) = current_sidebar_resolution(state) else {
        let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        crate::app::radar_movie::retire(state, wall_ms);
        let presentation = &mut state.match_state.match_presentation;
        presentation.radar_anim = None;
        presentation.radar_animation_source = None;
        presentation.radar_content_insets = None;
        return;
    };
    if state
        .match_state
        .match_presentation
        .radar_animation_source
        .as_ref()
        .is_some_and(|current| {
            current.requested_theme == resolved.requested_theme
                && current.actual_theme == resolved.actual_theme
                && &current.atlas == resolved.atlas.source_identity()
        })
    {
        return;
    }
    let identity = resolved.identity();
    let frames = resolved.atlas.radar_frames.clone();
    let [width, height] = resolved.atlas.radar_frame_size;
    let insets = resolved.atlas.radar_content_insets;
    let presentation = &mut state.match_state.match_presentation;
    if let Some(radar) = presentation.radar_anim.as_mut() {
        if !radar.replace_frames(
            &state.renderer.gpu,
            &state.renderer.batch_renderer,
            frames,
            width,
            height,
        ) {
            presentation.radar_anim = None;
        }
    } else {
        presentation.radar_anim = crate::render::radar::anim::RadarAnimState::new(
            &state.renderer.gpu,
            &state.renderer.batch_renderer,
            frames,
            width,
            height,
        );
        if let Some(radar) = presentation.radar_anim.as_mut() {
            //652E90 source installation does not request a new availability
            //edge. Hydrate the replacement owner silently; ordinary House
            //updates consume the typed cue through update_radar_state.
            let _ = radar.set_has_radar(presentation.has_radar);
        }
    }
    presentation.radar_animation_source = Some(identity);
    presentation.radar_content_insets = Some(insets);
    if presentation.radar_anim.is_none() {
        let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        crate::app::radar_movie::retire(state, wall_ms);
    }
}

/// Original gamemd.exe SHA256 1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c:
/// Full_Init @ 0x006877C9..0x0068781F resolves Basic.Player with 0x0050C170,
/// reads House.Country+0x34 -> Country.Side+0xBC, then installs that side for
/// InitSideMixFiles @ 0x00534FA0. Radar @ 0x00652E90 and resource selection
/// @ 0x0072FA10 consume the installed side. Map House Side/Country text is
/// not a second presentation authority. The executed stock House bindings
/// are in tools/input_oracle/campaign_start_houses.json; they establish
/// Americans/0 and Russians/1, without certifying native raster output.
pub(crate) fn sidebar_theme_for_owner(
    simulation: Option<&crate::sim::world::Simulation>,
    owner: &str,
) -> Option<crate::render::sidebar_chrome::SidebarTheme> {
    let simulation = simulation?;
    let live_house = crate::sim::house_state::house_state_for_owner(
        &simulation.houses,
        owner,
        &simulation.interner,
    )?;
    match live_house.side_index {
        0 => Some(crate::render::sidebar_chrome::SidebarTheme::Allied),
        1 => Some(crate::render::sidebar_chrome::SidebarTheme::Soviet),
        2 => Some(crate::render::sidebar_chrome::SidebarTheme::Yuri),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Render pass creation
// ---------------------------------------------------------------------------

/// Create the main render pass with depth buffer and clear.
pub(crate) fn begin_main_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    view: &'a wgpu::TextureView,
    depth_view: &'a wgpu::TextureView,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Main Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(crate::app::presentation::render::CLEAR_COLOR),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth_view,
            stencil_ops: None,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(crate::render::native_z::STORED_DEPTH_CLEAR),
                store: wgpu::StoreOp::Store,
            }),
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    })
}

/// Resume the tactical composition after a destination-dependent encoded-byte
/// surface edit. Both attachments retain the work produced by the first pass.
pub(crate) fn begin_main_load_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    view: &'a wgpu::TextureView,
    depth_view: &'a wgpu::TextureView,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Main Pass (resume after combat lights)"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth_view,
            stencil_ops: None,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            }),
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    })
}

#[cfg(test)]
mod tests {
    use super::shows_alt_cameo;
    use crate::sim::house_state::HouseState;
    use crate::sim::intern::StringInterner;
    use crate::ui::sidebar::SidebarTab;
    use crate::ui::sidebar::cameo_order::{Cameo, CameoId, CameoKey, CameoStrips};
    use crate::ui::sidebar::gadget_flash::SidebarGadgetState;

    /// The two native relocation transitions, applied to the retained tab.
    ///
    /// Recalculate 6A68AF..6A6930 and AddCameo 6A6432 both scan strips 0..3,
    /// disable the button of the strip that just emptied and move ActiveTab only
    /// when the first populated strip is not already active — a strip that was
    /// already empty reports neither transition.
    #[test]
    fn native_tab_relocation_follows_the_first_populated_strip() {
        let mut interner = StringInterner::new();
        let infantry = interner.intern("TESTSOLDIER");
        let building = interner.intern("TESTYARD");
        let cameo = |id, tab| Cameo {
            id: CameoId::Object(id),
            tab,
            available: true,
            key: CameoKey::Techno {
                own_side: true,
                considered_aircraft: false,
                naval: false,
                tech_level: 1,
                cost: 100,
                name: "Test Type".to_string(),
            },
        };
        let mut strips = CameoStrips::default();
        let mut active = SidebarTab::Building;
        let mut gadgets = SidebarGadgetState::default();
        let reconcile = |strips: &mut CameoStrips,
                         candidates: &[Cameo],
                         active: &mut SidebarTab,
                         gadgets: &mut SidebarGadgetState| {
            let reconciled = strips.reconcile_fixture(candidates, 3, true);
            super::apply_cameo_transitions(reconciled.transitions(), active, gadgets);
        };

        // An entry arriving into an empty sidebar selects its own strip.
        reconcile(
            &mut strips,
            &[cameo(infantry, SidebarTab::Infantry)],
            &mut active,
            &mut gadgets,
        );
        assert_eq!(active, SidebarTab::Infantry);
        assert!(!gadgets.tab_disabled[SidebarTab::Infantry.tab_index()]);

        // A later addition into a populated sidebar moves nothing.
        reconcile(
            &mut strips,
            &[
                cameo(infantry, SidebarTab::Infantry),
                cameo(building, SidebarTab::Building),
            ],
            &mut active,
            &mut gadgets,
        );
        assert_eq!(active, SidebarTab::Infantry);

        // Dropping the infantry entry empties its strip: the button disables and
        // the active tab follows the first populated strip.
        let mut pruned = vec![
            cameo(infantry, SidebarTab::Infantry),
            cameo(building, SidebarTab::Building),
        ];
        pruned[0].available = false;
        reconcile(&mut strips, &pruned, &mut active, &mut gadgets);
        assert_eq!(active, SidebarTab::Building);
        assert!(gadgets.tab_disabled[SidebarTab::Infantry.tab_index()]);

        // Nothing populated: the disabled button stays, the tab does not move.
        let mut emptied = pruned.clone();
        emptied[1].available = false;
        reconcile(&mut strips, &emptied, &mut active, &mut gadgets);
        assert_eq!(active, SidebarTab::Building);
        assert!(gadgets.tab_disabled[SidebarTab::Building.tab_index()]);
    }

    /// The first AddCameo into an empty sidebar selects its strip; a later
    /// SuperWeapon AddCameo does not reselect the lowest populated strip.
    ///
    /// Native 6A6300 performs the relocation on the individual insertion only
    /// when the sidebar was empty before that insertion. House4F927C..4F92FD
    /// inserts ordinary cameos before granting Supers, so an Infantry entry
    /// can select tab 2 before a later Defense superweapon is added. The
    /// executed control `native-tabs3-report.json` also shows an AddCameo into
    /// an already-populated sidebar leaving ActiveTab unchanged.
    #[test]
    fn later_super_addition_does_not_replace_the_first_empty_sidebar_tab() {
        let mut interner = StringInterner::new();
        let infantry = interner.intern("TESTSOLDIER");
        let super_weapon = interner.intern("TESTSPECIAL");
        let candidates = [
            Cameo {
                id: CameoId::Object(infantry),
                tab: SidebarTab::Infantry,
                available: true,
                key: CameoKey::Techno {
                    own_side: true,
                    considered_aircraft: false,
                    naval: false,
                    tech_level: 1,
                    cost: 100,
                    name: "Test Infantry".to_string(),
                },
            },
            Cameo {
                id: CameoId::SuperWeapon(super_weapon),
                tab: SidebarTab::Defense,
                available: true,
                key: CameoKey::SuperWeapon {
                    recharge_frames: 30,
                    name: "Test Superweapon".to_string(),
                },
            },
        ];
        let mut strips = CameoStrips::default();
        let reconciled = strips.reconcile_fixture(&candidates, 3, true);
        let mut active = SidebarTab::Building;
        let mut gadgets = SidebarGadgetState::default();

        super::apply_cameo_transitions(reconciled.transitions(), &mut active, &mut gadgets);

        assert_eq!(
            active,
            SidebarTab::Infantry,
            "the later Defense superweapon is added after the Infantry tab was selected"
        );
    }

    /// Retail rules and art: whose cameo `TechnoTypeClass::GetCameo`
    /// switches after each factory infiltration, and the image section the
    /// `AltCameo=` comes from (MTNK's `Image=GTNK`).
    #[test]
    fn retail_alt_cameos_follow_the_stolen_veterancy() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = &retail.rules;
        let id = crate::sim::intern::InternedId::default();
        let shows = |house: &HouseState, kind: &str| {
            shows_alt_cameo(house, rules.object(kind).expect("a retail type"))
        };
        let mut barracks = HouseState::new(id, 0, None, true, 0, 10);
        barracks.grant_veteran_infantry();
        assert!(shows(&barracks, "E1"));
        assert!(!shows(&barracks, "SPY"), "Trainable=no");
        assert!(!shows(&barracks, "MTNK"));
        let mut war_factory = HouseState::new(id, 0, None, true, 0, 10);
        war_factory.grant_veteran_units();
        assert!(shows(&war_factory, "MTNK"));
        assert!(shows(&war_factory, "DEST"), "GetCameo does not ask Naval=");
        assert!(!shows(&war_factory, "ORCA"));
        assert!(!shows(&war_factory, "E1"));

        let alt = |kind: &str| {
            let image = &rules.object(kind).unwrap().image;
            rules.art().resolve_declared_alt_cameo_id(kind, image)
        };
        assert_eq!(alt("E1").as_deref(), Some("GIUICO"));
        assert_eq!(alt("MTNK").as_deref(), Some("GTNKUICO"));
        assert_eq!(
            alt("ORCA").as_deref(),
            Some("FALCUICO"),
            "declared, but GetCameo never draws an aircraft's"
        );
    }
}
