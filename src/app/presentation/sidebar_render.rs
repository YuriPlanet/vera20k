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
use crate::render::batch::BatchTexture;
use crate::sidebar::{self, SidebarView};
use crate::sim::production;

// Re-export instance builders so callers don't need to know about the split.
pub(crate) use crate::app::presentation::sidebar_build::{
    build_sidebar_cameo_instances, build_sidebar_chrome_instances, build_sidebar_instances,
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
    let credits = production::credits_for_owner(sim, &owner_name);
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

/// Reconcile state-derived sidebar inputs and replace the retained immutable
/// projection. This is called only from explicit simulation/input/lifecycle
/// transitions, never from a view consumer.
pub(crate) fn refresh_sidebar_projection(state: &mut AppState) {
    refresh_radar_animation_source(state);
    let mut spec = crate::sidebar::SidebarChromeLayoutSpec::for_theme(current_sidebar_theme(state));
    if let Some(atlas) = current_sidebar_chrome(state) {
        spec.side2_height = atlas.side2.pixel_size[1];
        spec.side3_height = atlas.side3.pixel_size[1];
    }
    state.match_state.match_presentation.sidebar_layout_spec = spec;
    let owner_name: String =
        preferred_local_owner_name(state).unwrap_or_else(|| "Americans".to_string());
    let Some((
        mut build_options,
        mut queue_items,
        mut ready_buildings,
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
        let sw_views = if sim.session.game_options.super_weapons {
            crate::sim::superweapon::superweapon_views_for_owner(sim, rules, &owner_iid)
        } else {
            Vec::new()
        };
        let (power_produced, power_drained) =
            production::power_balance_for_owner(sim, rules, &owner_name);
        Some((
            production::build_options_for_owner(sim, rules, &owner_name),
            production::queue_view_for_owner(sim, rules, &owner_name),
            production::ready_buildings_for_owner(sim, rules, &owner_name),
            producer_focus,
            production::credits_for_owner(sim, &owner_name),
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

    // `SidebarClass::AddCameo 0x006A63D6..0x006A6415`: a build cameo the
    // strip did not hold (`visible_in_sidebar` is the strip entry set; the
    // superweapon strip is the `RTTI == 0x1F` exclusion) speaks
    // `EVA_NewConstructionOptions` once the scenario-init nesting counter
    // is back to zero — the first projection of a match is that window.
    let cameos: std::collections::BTreeSet<_> = build_options
        .iter()
        .filter(|opt| opt.visible_in_sidebar())
        .map(|opt| opt.type_id)
        .collect();
    if state
        .match_state
        .match_presentation
        .sidebar_projection
        .note_cameos(cameos)
    {
        crate::app::input::dispatch::push_local_eva(
            state,
            crate::app::input::sidebar_eva::EVA_NEW_CONSTRUCTION_OPTIONS,
        );
    }

    // Resolve CSF display names (e.g., "Name:MTNK" → "Grizzly Battle Tank").
    if let Some(csf) = &state.process_assets.csf {
        for opt in &mut build_options {
            opt.display_name = resolve_csf_name(csf, &opt.display_name);
        }
        for item in &mut queue_items {
            item.display_name = resolve_csf_name(csf, &item.display_name);
        }
        for ready in &mut ready_buildings {
            ready.display_name = resolve_csf_name(csf, &ready.display_name);
        }
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
    sync_targeting_mode(
        &mut state.match_state.input.targeting_mode,
        &mut state.match_state.input.building_placement_preview,
        &ready_buildings,
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)
            .map(|s| &s.interner),
    );
    // App targeting state -> the sidebar-owned armed projection (F06 seam).
    let armed_entry = state
        .match_state
        .input
        .targeting_mode
        .as_ref()
        .map(|mode| match mode {
            crate::app::types::TargetingMode::BuildingPlacement(section) => {
                sidebar::ArmedSidebarEntry::BuildingPlacement(section.clone())
            }
            crate::app::types::TargetingMode::SuperWeapon(section) => {
                sidebar::ArmedSidebarEntry::SuperWeapon(section.clone())
            }
        });
    let mut view = sidebar::build_sidebar_view_with_spec(
        state.match_state.match_presentation.sidebar_layout_spec,
        state.render_width() as f32,
        state.render_height() as f32,
        state.match_state.match_presentation.active_sidebar_tab,
        display_credits,
        power_produced,
        power_drained,
        tab_btn_size,
        &queue_items,
        &build_options,
        &ready_buildings,
        armed_entry.as_ref(),
        &producer_focus,
        state.match_state.match_presentation.sidebar_scroll_rows,
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
            .sidebar_scroll_rows_parked,
    );
    if let Some(selected) = view.tabs.iter().find(|tab| tab.active) {
        crate::app::input::dispatch::apply_sidebar_action(
            state,
            sidebar::SidebarAction::SelectTab(selected.tab),
        );
    }
    state.match_state.match_presentation.sidebar_scroll_rows = view.scroll_rows;
    // Publish the same strip-derived state to input and Flash_AI. Native
    //6A6472 enables after Add;6A6820 disables after the last entry is removed.
    for tab in &view.tabs {
        let gadgets = &mut state.match_state.match_presentation.sidebar_gadget_state;
        gadgets.tab_disabled[tab.tab.tab_index()] = tab.disabled;
        gadgets.tab_flashes[tab.tab.tab_index()].disabled = tab.disabled;
    }
    if let Some(atlas) = state
        .match_state
        .match_presentation
        .sidebar_cameo_atlas
        .as_ref()
    {
        for item in &mut view.items {
            item.has_cameo_art = atlas.get(&item.type_id).is_some();
        }
    }
    state
        .match_state
        .match_presentation
        .sidebar_projection
        .replace_view(Some(view));
}

/// Drops a building placement whose building is no longer ready. A selected
/// Super is not the sidebar's to drop: its writers are
/// `super_selection::follow_selection_writes`'s.
pub(crate) fn sync_targeting_mode(
    targeting_mode: &mut Option<crate::app::types::TargetingMode>,
    building_placement_preview: &mut Option<crate::sim::production::BuildingPlacementPreview>,
    ready_buildings: &[production::ReadyBuildingView],
    interner: Option<&crate::sim::intern::StringInterner>,
) {
    let still_valid = match targeting_mode.as_ref() {
        None | Some(crate::app::types::TargetingMode::SuperWeapon(_)) => true,
        Some(crate::app::types::TargetingMode::BuildingPlacement(armed)) => {
            ready_buildings.iter().any(|ready| {
                interner.map_or(false, |i| {
                    i.resolve(ready.type_id).eq_ignore_ascii_case(armed)
                })
            })
        }
    };
    if !still_valid {
        *targeting_mode = None;
        *building_placement_preview = None;
    }
}

// ---------------------------------------------------------------------------
// Minimap interaction
// ---------------------------------------------------------------------------

pub(crate) fn is_cursor_over_minimap(state: &AppState) -> bool {
    // Minimap interaction disabled when radar is not online.
    let minimap_visible: bool = state
        .match_state
        .match_presentation
        .radar_anim
        .as_ref()
        .map_or(true, |ra| ra.is_minimap_visible());
    if !minimap_visible {
        return false;
    }
    let Some(minimap) = &state.match_state.match_presentation.minimap else {
        return false;
    };
    let rect = active_minimap_screen_rect(state);
    minimap.contains_screen_point_in_rect(
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
        rect.x,
        rect.y,
        rect.w,
        rect.h,
    )
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

pub(crate) fn update_camera_from_minimap_cursor(state: &mut AppState) {
    let rect = active_minimap_screen_rect(state);
    let Some(minimap) = state.match_state.match_presentation.minimap.as_ref() else {
        return;
    };
    let runtime = state.match_state.sim_runtime.as_ref();
    let native_facts = runtime.and_then(|runtime| {
        let view = runtime.view();
        Some((
            view.entities(),
            view.resolved_terrain()?,
            view.playfield_map_size()?,
        ))
    });
    let (tactical_w, tactical_h) = crate::app::input::camera::tactical_viewport_size_px(
        state.render_width(),
        state.render_height(),
    );
    let native_target = native_facts.and_then(|(entities, terrain, map_size)| {
        minimap.native_click_target_in_rect(
            state.match_state.input.cursor_x,
            state.match_state.input.cursor_y,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            entities,
            terrain,
            map_size,
            (tactical_w as i32, tactical_h as i32),
        )
    });
    if let Some(target) = native_target {
        // `0x00653EA0 -> FUN_006D6070` writes current and desired viewport
        // together from the complete CellClass center XYZ. The camera owner
        // discards any pending scroll request when setting the absolute view.
        let (x, y, z) = target.world_leptons;
        crate::app::input::camera::center_camera_on_lepton_point(state, x, y, z);
        return;
    }
    if minimap.has_playfield_authority() {
        // A live map whose generated surface is unavailable must not silently
        // approximate the click through the old normalized 200x200 transform.
        return;
    }

    // Mapless/headless presentation fixture adapter only.
    let sw = state.render_width() as f32;
    let sh = state.render_height() as f32;
    let (tactical_w, tactical_h) = crate::app::input::camera::tactical_viewport_size_px(
        state.render_width(),
        state.render_height(),
    );
    let z = state.match_state.input.zoom_level;
    let (cx, cy) = minimap.camera_top_left_for_screen_point_in_rect(
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
        tactical_w as f32 / z,
        tactical_h as f32 / z,
        rect.x,
        rect.y,
        rect.w,
        rect.h,
    );
    crate::app::input::camera::set_camera_position(state, (cx, cy));
    crate::app::input::camera::clamp_camera_to_playable_area(state, sw, sh);
}

/// Live generated-primary screen rectangle used by the retained input region.
/// Centered letterbox margins are not part of RadarClass's click surface.
pub(crate) fn active_minimap_content_screen_rect(state: &AppState) -> crate::sidebar::Rect {
    let aperture = active_minimap_screen_rect(state);
    let Some(minimap) = state.match_state.match_presentation.minimap.as_ref() else {
        return crate::sidebar::Rect {
            x: aperture.x,
            y: aperture.y,
            w: 0.0,
            h: 0.0,
        };
    };
    let Some([x, y, w, h]) =
        minimap.content_screen_rect_in_rect(aperture.x, aperture.y, aperture.w, aperture.h)
    else {
        return crate::sidebar::Rect {
            x: aperture.x,
            y: aperture.y,
            w: 0.0,
            h: 0.0,
        };
    };
    crate::sidebar::Rect { x, y, w, h }
}

pub(crate) fn active_minimap_screen_rect(state: &AppState) -> crate::sidebar::Rect {
    let sw = state.render_width() as f32;
    let sh = state.render_height() as f32;
    if current_sidebar_chrome(state).is_some() {
        crate::sidebar::radar_minimap_rect_with_spec(
            sw,
            state.match_state.match_presentation.sidebar_layout_spec,
        )
    } else {
        let (x, y, w, h) = crate::render::minimap::default_minimap_rect(sh);
        crate::sidebar::Rect { x, y, w, h }
    }
}

// ---------------------------------------------------------------------------
// Chrome / theme helpers
// ---------------------------------------------------------------------------

pub(crate) fn current_sidebar_chrome_texture(state: &AppState) -> Option<&BatchTexture> {
    current_sidebar_chrome(state).map(|atlas| &atlas.texture)
}

pub(crate) fn current_sidebar_gclock_texture(state: &AppState) -> Option<&BatchTexture> {
    current_sidebar_chrome(state).and_then(|atlas| atlas.gclock_texture.as_ref())
}

pub(crate) fn current_sidebar_theme(
    state: &AppState,
) -> crate::render::sidebar_chrome::SidebarTheme {
    preferred_local_owner_name(state)
        .and_then(|owner| {
            sidebar_theme_for_owner_sources(
                state
                    .match_state
                    .sim_runtime
                    .as_ref()
                    .map(|rt| &rt.simulation),
                &state.match_state.match_presentation.house_roster,
                &owner,
            )
        })
        .unwrap_or(crate::render::sidebar_chrome::SidebarTheme::Allied)
}

/// Shared installed-owner and atlas fallback projection for chrome and radar.
/// The map loader calls the final sidebar refresh only after installing the
/// new simulation, roster and pinned owner; this helper never reads old load inputs.
pub(crate) fn project_sidebar_source<'a, T>(
    simulation: Option<&crate::sim::world::Simulation>,
    roster: &crate::map::houses::HouseRoster,
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
        .and_then(|owner| sidebar_theme_for_owner_sources(simulation, roster, owner))
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
        &state.match_state.match_presentation.house_roster,
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
        presentation.radar_anim = crate::render::radar_anim::RadarAnimState::new(
            &state.renderer.gpu,
            &state.renderer.batch_renderer,
            frames,
            width,
            height,
        );
        if let Some(radar) = presentation.radar_anim.as_mut() {
            radar.set_has_radar(presentation.has_radar);
        }
    }
    presentation.radar_animation_source = Some(identity);
    presentation.radar_content_insets = Some(insets);
}

pub(crate) fn sidebar_theme_for_owner_sources(
    simulation: Option<&crate::sim::world::Simulation>,
    house_roster: &crate::map::houses::HouseRoster,
    owner: &str,
) -> Option<crate::render::sidebar_chrome::SidebarTheme> {
    if let Some(house) = house_roster
        .houses
        .iter()
        .find(|house| house.name.eq_ignore_ascii_case(owner))
    {
        let side = house
            .side
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let country = house
            .country
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase();

        if side.contains("yuri") || country.contains("yuri") {
            return Some(crate::render::sidebar_chrome::SidebarTheme::Yuri);
        }
        if side.contains("soviet")
            || matches!(
                country.as_str(),
                "russia" | "iraq" | "cuba" | "libya" | "soviet"
            )
        {
            return Some(crate::render::sidebar_chrome::SidebarTheme::Soviet);
        }
        return Some(crate::render::sidebar_chrome::SidebarTheme::Allied);
    }

    // Map-loaded houses can still default a missing Side= to Allied. Preserve
    // the existing roster decision until that producer is exact; ordinary
    // explicit launch names miss the map roster and carry the resolved live
    // side. A deliberate name collision therefore keeps the roster decision.
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
// CSF display name resolution
// ---------------------------------------------------------------------------

/// Resolve a display name through the CSF string table.
///
/// Rules `Name=` values are CSF keys (e.g., `"Name:MTNK"`). Retail emits its
/// visible `MISSING:'<key>'` marker when the initialized table lacks a key.
fn resolve_csf_name(csf: &crate::assets::csf_file::CsfFile, name: &str) -> String {
    csf.text(name).into_owned()
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
