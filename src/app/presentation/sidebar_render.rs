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
use crate::app::sidebar_projection::SidebarViewInputs;
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
    let Some(owner_name) = preferred_local_owner_name(state) else {
        return;
    };
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return;
    };
    let credits = counter_credits(sim, &owner_name);
    #[cfg(feature = "phobos")]
    let (max_step, smooth) = {
        let ui = &state.match_state.match_presentation.sidebar_indicators;
        (ui.credits_max_step, ui.credits_smooth)
    };
    #[cfg(not(feature = "phobos"))]
    let (max_step, smooth) = (143, true);
    let Some(tick) = state
        .match_state
        .match_presentation
        .sidebar_projection
        .advance_credits(&owner_name, credits, max_step, smooth)
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

/// Apply one reconcile pass's native initialization/AddCameo/Recalculate
/// transitions. Sidebar Init6A5411 disables empty buttons on the first pass.
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
/// transitions, never from a view consumer. It derives the simulation's
/// inputs afresh, so a transition that wrote the world outside a frame needs
/// no revision of its own.
pub(crate) fn refresh_sidebar_projection(state: &mut AppState) {
    refresh_projection(state, false);
}

/// The gadget idle tick's republish on every presentation pass: the same
/// refresh, reusing the simulation inputs kept for the world's revision
/// ([`crate::app::sidebar_projection::SidebarSimInputs`]).
pub(crate) fn republish_sidebar_projection(state: &mut AppState) {
    refresh_projection(state, true);
}

fn refresh_projection(state: &mut AppState, reuse_sim_inputs: bool) {
    refresh_radar_animation_source(state);
    let theme = current_sidebar_theme(state);
    #[cfg(feature = "phobos")]
    let gdi_positions = state.rules().map_or(
        theme == crate::render::sidebar_chrome::SidebarTheme::Allied,
        |rules| rules.side_gdi_positions(current_sidebar_side(state)),
    );
    #[cfg(not(feature = "phobos"))]
    let gdi_positions = theme == crate::render::sidebar_chrome::SidebarTheme::Allied;
    let mut spec =
        crate::ui::sidebar::SidebarChromeLayoutSpec::for_theme_and_positions(theme, gdi_positions);
    if let Some(atlas) = current_sidebar_chrome(state) {
        spec.side2_height = atlas.side2.pixel_size[1];
        spec.side3_height = atlas.side3.pixel_size[1];
    }
    state.match_state.match_presentation.sidebar_layout_spec = spec;
    // The sidebar lays out on its own surface (`ui::sidebar::SidebarFrame`).
    let (screen_w, screen_h) = state.sidebar_frame().layout_screen();
    let visible_rows =
        sidebar::compute_layout_with_spec(spec, screen_w, screen_h, 0).side2_tile_count;
    let Some(owner_name) = preferred_local_owner_name(state) else {
        state
            .match_state
            .match_presentation
            .sidebar_projection
            .replace_view(None);
        return;
    };
    let inputs = {
        let match_state = &mut state.match_state;
        let Some(runtime) = match_state.sim_runtime.as_ref() else {
            match_state
                .match_presentation
                .sidebar_projection
                .replace_view(None);
            return;
        };
        match_state
            .match_presentation
            .sidebar_projection
            .sim_inputs(
                &runtime.simulation,
                &runtime.resources.rules,
                &owner_name,
                reuse_sim_inputs,
            )
    };

    let runtime = state
        .match_state
        .sim_runtime
        .as_ref()
        .expect("sidebar runtime");
    let (sim, rules) = (&runtime.simulation, &runtime.resources.rules);
    let owner = sim.interner().get(&owner_name).unwrap_or_default();
    let seed_additions = state
        .match_state
        .match_presentation
        .sidebar_projection
        .needs_cameo_seed(owner)
        .then(|| {
            production::initial_construction_options(sim, state.rules().expect("sidebar rules"))
        })
        .unwrap_or_default();
    // Phobos adds a superweapon to the left sidebar first and skips its right
    // sidebar cameo (`SidebarClass_AddCameo` returns false once accepted).
    #[cfg(feature = "phobos")]
    let sw_plan = crate::app::presentation::sw_sidebar::plan(state, owner, &inputs.sw_views);
    #[cfg(feature = "phobos")]
    let right_sw_views: Vec<crate::sim::superweapon::SuperWeaponView> = {
        let left: std::collections::HashSet<&str> = sw_plan
            .1
            .iter()
            .map(|entry| entry.type_id.as_str())
            .collect();
        inputs
            .sw_views
            .iter()
            .filter(|view| !left.contains(sim.interner().resolve(view.type_id)))
            .cloned()
            .collect()
    };
    #[cfg(not(feature = "phobos"))]
    let right_sw_views = inputs.sw_views.clone();
    let cameos = state
        .match_state
        .match_presentation
        .sidebar_projection
        .cameo_candidates(
            sim,
            rules,
            state.process_assets.csf.as_ref(),
            owner,
            &owner_name,
            &inputs,
            &right_sw_views,
            reuse_sim_inputs,
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
        .set_cameo_frame(sim.session().binary_frame() as i32);
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
        .displayed_credits_or_seed(&owner_name, inputs.credits);
    let (
        tab_btn_size,
        repair_btn_size,
        sell_btn_size,
        scroll_down_btn_size,
        scroll_up_btn_size,
        top_btn_sizes,
    ) = {
        let size = |entry: Option<&crate::render::sidebar_chrome::SidebarChromeEntry>| {
            entry.map(|entry| entry.pixel_size)
        };
        let atlas = current_sidebar_chrome(state);
        (
            size(atlas.and_then(|atlas| atlas.tab_frames[0][0].as_ref())),
            size(atlas.and_then(|atlas| atlas.repair_frames[0].as_ref())),
            size(atlas.and_then(|atlas| atlas.sell_frames[0].as_ref())),
            size(atlas.and_then(|atlas| atlas.scroll_down_frames[0].as_ref())),
            size(atlas.and_then(|atlas| atlas.scroll_up_frames[0].as_ref())),
            std::array::from_fn::<_, 2, _>(|i| {
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
    let view_inputs = SidebarViewInputs {
        spec: state.match_state.match_presentation.sidebar_layout_spec,
        screen: (screen_w, screen_h),
        tab: state.match_state.match_presentation.active_sidebar_tab,
        credits: display_credits,
        sim: inputs,
        button_sizes: [
            tab_btn_size,
            repair_btn_size,
            sell_btn_size,
            scroll_down_btn_size,
            scroll_up_btn_size,
            top_btn_sizes[0],
            top_btn_sizes[1],
        ],
        armed: armed_entry,
        supers: right_sw_views,
        gadgets: state
            .match_state
            .match_presentation
            .sidebar_gadget_state
            .clone(),
    };
    // The gadget refresh runs this on every service pass, several per game
    // frame; while the inputs, strips and Factory links all hold, the view
    // it would build equals the retained one. Debug builds build it anyway
    // and compare.
    let holds = state
        .match_state
        .match_presentation
        .sidebar_projection
        .view_holds_for(&view_inputs);
    if !holds || crate::sim::live_read_check_enabled() {
        let view = build_view(state, &view_inputs, owner);
        let projection = &mut state.match_state.match_presentation.sidebar_projection;
        if holds {
            assert_eq!(
                projection.view(),
                Some(&view),
                "the sidebar view changed while its inputs held"
            );
        }
        projection.replace_built_view(view, view_inputs);
    }
    #[cfg(feature = "phobos")]
    {
        let sw = &mut state.match_state.match_presentation.sw_sidebar;
        sw.layout = sw_plan.0;
        sw.entries = sw_plan.1;
    }
}

/// The view a refresh retains, built from `inputs` and the projection's
/// strips and Factory links, with each cameo's art and drawn state.
fn build_view(
    state: &mut AppState,
    inputs: &SidebarViewInputs,
    owner: crate::sim::intern::InternedId,
) -> SidebarView {
    let mut view = sidebar::build_sidebar_view_with_spec(
        inputs.spec,
        inputs.screen.0,
        inputs.screen.1,
        inputs.tab,
        inputs.credits,
        inputs.sim.power_produced,
        inputs.sim.power_drained,
        inputs.button_sizes[0],
        &inputs.sim.queue_items,
        &inputs.sim.build_options,
        &inputs.sim.ready_buildings,
        inputs.armed.as_ref(),
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)
            .map(|sim| sim.interner()),
        &inputs.supers,
        &inputs.gadgets,
        inputs.button_sizes[1],
        inputs.button_sizes[2],
        inputs.button_sizes[3],
        inputs.button_sizes[4],
        [inputs.button_sizes[5], inputs.button_sizes[6]],
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
        .set_scroll_row(inputs.tab, view.scroll_rows);
    if let Some(atlas) = state
        .match_state
        .match_presentation
        .sidebar_cameo_atlas
        .as_ref()
    {
        let simulation = state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation);
        let house = simulation.and_then(|sim| sim.houses().get(&owner));
        let rules = state.rules();
        for item in &mut view.items {
            item.alt_cameo = !item.is_superweapon
                && house.is_some_and(|house| {
                    rules.is_some_and(|rules| {
                        rules.object(&item.type_id).is_some_and(|obj| {
                            let country = simulation
                                .map_or("", |sim| sim.interner().resolve(house.house_type_id()));
                            shows_alt_cameo(
                                house,
                                obj,
                                rules,
                                country,
                                atlas.has_alternate(&item.type_id),
                            )
                        })
                    })
                });
            item.has_cameo_art = if item.is_superweapon {
                atlas.superweapon(&item.type_id)
            } else {
                atlas.cameo(&item.type_id, item.alt_cameo)
            }
            .is_some();
        }
    }
    // The rebuilt view takes its drawn cameo state from the same projection
    // write as every later frame, so it is never built from other rules.
    if let Some(runtime) = state.match_state.sim_runtime.as_ref() {
        let sim = &runtime.simulation;
        let armed_building = inputs
            .armed
            .as_ref()
            .and_then(sidebar::ArmedSidebarEntry::as_building_placement)
            .and_then(|name| sim.interner().get(name));
        state
            .match_state
            .match_presentation
            .sidebar_projection
            .apply_cameo_draw_state(
                &mut view,
                sim,
                &runtime.resources.rules,
                owner,
                armed_building,
            );
    }
    view
}

/// The per-frame part of the projection: each retained object cameo's drawn
/// state from the live simulation (`apply_cameo_draw_state`).
/// `StripClass::Draw @ 0x006A9540` asks `FindFactory(1, 1, 1)`,
/// `CanBuild(type, 0, 0)` and `CheckBuildLimit` and reads the linked Factory
/// for every entry it draws, so a restriction that appears or clears between
/// two House rechecks must reach the very next frame. Membership and the House
/// pass stay with [`refresh_sidebar_projection`].
/// RepaintForRemovedObject 6A5F20, RepaintSidebar 6A60A0 and
/// RepaintSidebarFor2Frames 6A60E0 only set redraw flags and RedrawUntilFrame;
/// VERA draws the sidebar every frame, so they have no port.
pub(crate) fn refresh_sidebar_cameo_draw_state(state: &mut AppState) {
    let Some(owner_name) = preferred_local_owner_name(state) else {
        return;
    };
    let AppState { match_state, .. } = state;
    let Some(runtime) = match_state.sim_runtime.as_ref() else {
        return;
    };
    let sim = &runtime.simulation;
    let owner = sim.interner().get(&owner_name).unwrap_or_default();
    let armed_building = match_state
        .input
        .targeting_mode
        .as_ref()
        .and_then(crate::app::types::TargetingMode::as_building_placement)
        .and_then(|name| sim.interner().get(name));
    match_state
        .match_presentation
        .sidebar_projection
        .refresh_cameo_draw_state(sim, &runtime.resources.rules, owner, armed_building);
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
/// Ares1003D800 additionally admits the current Country's retained lists.
/// An undeploying Building uses the Unit-list branch, comparing its own
/// BuildingType pointer, not UndeploysInto; that cannot match an INI-produced
/// UnitType list. Phobos fixes that arm (see
/// [`undeploying_building_is_veteran`]). The spy arms read HouseState's
/// stock flags and the Ares SpyEffect veterancy (`sim::spy_infiltration`);
/// new Ares SpyEffect.Custom House gains are a separate chain.
///
/// [`SidebarCameoAtlas::cameo`]: crate::render::sidebar_cameo_atlas::SidebarCameoAtlas::cameo
fn shows_alt_cameo(
    house: &crate::sim::house_state::HouseState,
    obj: &crate::rules::object_type::ObjectType,
    rules: &crate::rules::ruleset::RuleSet,
    country: &str,
    has_alternate: bool,
) -> bool {
    use crate::rules::object_type::ObjectCategory;
    if !has_alternate {
        return false;
    }
    if crate::rules::ARES_ENABLED {
        if obj.category == ObjectCategory::Building && obj.undeploys_into.is_some() {
            #[cfg(feature = "phobos")]
            return undeploying_building_is_veteran(house, obj, rules, country);
            #[cfg(not(feature = "phobos"))]
            return false;
        }
        if rules.country_starts_veteran(country, obj) {
            return true;
        }
        // Ares 3.0 1003D89A/1003D8DE/1003D911: a naval unit asks the
        // house's SpyEffect NavalVeterancy instead of the war factory's,
        // aircraft and non-undeploying buildings their own.
        #[cfg(feature = "ares")]
        {
            return obj.trainable
                && match obj.category {
                    ObjectCategory::Infantry => !obj.naval && house.veteran_infantry(),
                    ObjectCategory::Vehicle if obj.naval => house.naval_veterancy(),
                    ObjectCategory::Vehicle => house.veteran_units(),
                    ObjectCategory::Aircraft => house.aircraft_veterancy(),
                    ObjectCategory::Building => house.building_veterancy(),
                };
        }
        #[cfg(not(feature = "ares"))]
        {
            return obj.trainable
                && !obj.naval
                && match obj.category {
                    ObjectCategory::Infantry => house.veteran_infantry(),
                    ObjectCategory::Vehicle => house.veteran_units(),
                    ObjectCategory::Aircraft | ObjectCategory::Building => false,
                };
        }
    }
    obj.trainable
        && match obj.category {
            ObjectCategory::Infantry => house.veteran_infantry(),
            ObjectCategory::Vehicle => house.veteran_units(),
            ObjectCategory::Building => house.veteran_units() && obj.undeploys_into.is_some(),
            ObjectCategory::Aircraft => false,
        }
}

/// Phobos' fixed building arm of `TechnoTypeExt::CameoIsVeteran`: a building
/// that undeploys answers with its unit, so a `Factory=UnitType` infiltration
/// and the unit's `VeteranUnits` entry promote the building too. Ares instead
/// compared the BuildingType pointer against the unit list, which an INI list
/// can never match. VERA keeps one ground veteran flag, so the naval variant
/// Phobos reads separately cannot differ here.
#[cfg(feature = "phobos")]
fn undeploying_building_is_veteran(
    house: &crate::sim::house_state::HouseState,
    obj: &crate::rules::object_type::ObjectType,
    rules: &crate::rules::ruleset::RuleSet,
    country: &str,
) -> bool {
    if !obj.trainable {
        return false;
    }
    if house.veteran_units() {
        return true;
    }
    obj.undeploys_into
        .as_deref()
        .and_then(|unit| rules.object(unit))
        .is_some_and(|unit| rules.country_starts_veteran(country, unit))
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
            view.simulation().map_size_diamond()?,
        ))
    });
    let (tactical_w, tactical_h) = crate::app::input::camera::tactical_viewport_size(state);
    native_facts.and_then(|(simulation, rules, terrain, map_size)| {
        minimap.native_click_target_in_rect(
            cursor_x,
            cursor_y,
            sidebar_cursor_centre(state),
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

/// The order/cursor target precedes the camera branch's viewport clamp in
/// RadarClass::Input (`0x00653ABC` versus `0x00653D7A`).
pub(crate) fn minimap_input_target(
    state: &AppState,
) -> Option<crate::render::radar::NativeRadarInputTarget> {
    let rect = active_minimap_screen_rect(state);
    let runtime = state.match_state.sim_runtime.as_ref()?;
    state
        .match_state
        .match_presentation
        .minimap
        .as_ref()?
        .native_input_target_in_rect(
            sidebar_cursor(state),
            sidebar_cursor_centre(state),
            [rect.x, rect.y, rect.w, rect.h],
            &runtime.simulation,
            &runtime.resources.rules,
        )
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

/// The centre of the window pixel under the cursor, in the same layout space.
/// The cursor sits on that pixel's corner in frame pixels; the HD radar picks
/// its own finer pixels at the centre.
fn sidebar_cursor_centre(state: &AppState) -> (f32, f32) {
    let half = 0.5
        / state
            .match_zoom_pass()
            .map_or(1.0, |pass| pass.geometry().scale());
    state.sidebar_frame().point_to_layout(
        state.match_state.input.cursor_x + half,
        state.match_state.input.cursor_y + half,
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

/// Resolve every Phobos sidebar indicator for this frame and hand the list to
/// the sidebar text lane. gamemd's hook runs inside the credits tick, so the
/// caller is the per-frame pass that also ticks the power bar
/// (`match_runtime::sim_tick`), not the cameo-driven projection refresh.
#[cfg(feature = "phobos")]
pub(crate) fn refresh_sidebar_indicators(state: &mut AppState) {
    let resolved = (|| {
        let sim = state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)?;
        let rules = state.rules()?;
        let owner_name = preferred_local_owner_name(state)?;
        let owner = sim.interner().get(&owner_name).unwrap_or_default();
        let (produced, drained) = production::power_balance_for_owner(sim, rules, &owner_name);
        let presentation = &state.match_state.match_presentation;
        let side = current_sidebar_side(state);
        // HouseExt's two counts, walked over the store once per frame.
        let (harvesters_active, harvesters_total) = sim.harvester_counts(rules, owner);
        // Native draws the harvester counter before the power delta, and this is
        // also the lane's append order.
        let mut indicators: Vec<_> = [
            harvester_counter_indicator(
                sim,
                rules,
                side,
                owner,
                &presentation.sidebar_indicators,
                presentation.show_harvester_counter,
                crate::app::presentation::sidebar_text::credits_tint(current_sidebar_theme(state)),
                harvesters_active,
                harvesters_total,
            ),
            power_delta_indicator(
                sim,
                rules,
                side,
                owner,
                &presentation.sidebar_indicators,
                presentation.show_power_delta,
                produced,
                drained,
            ),
            weeds_counter_indicator(
                sim,
                rules,
                side,
                owner,
                &presentation.sidebar_indicators,
                presentation.show_weeds_counter,
                crate::app::presentation::sidebar_text::credits_tint(current_sidebar_theme(state)),
            ),
        ]
        .into_iter()
        .flatten()
        .collect();
        // `0x6A6EB1` is a separate native drawing site inside the sidebar's
        // own draw pass, so its strips paint over the credits-tick text.
        indicators.extend(producing_progress_indicators(
            &presentation.sidebar_indicators,
            &rules.side_producing_progress(side),
            producing_progress_columns(sim, owner),
            rules.side_gdi_positions(side),
            presentation.sidebar_layout_spec.sidebar_width.round() as i32,
            crate::app::presentation::sidebar_text::credits_tint(current_sidebar_theme(state)),
        ));
        Some(indicators)
    })();
    let presentation = &mut state.match_state.match_presentation;
    match resolved {
        Some(resolved) => presentation.sidebar_indicator_text = resolved,
        // No local house: the hook never runs for this player.
        None => presentation.sidebar_indicator_text.clear(),
    }
}

// ---------------------------------------------------------------------------
// Chrome / theme helpers
// ---------------------------------------------------------------------------

/// Keep VERA's smooth sidebar (`render::sidebar_smoothing`) at this frame's
/// draw scale, or drop its copies when the option is off or the sidebar draws
/// at its own pixels.
pub(crate) fn prepare_smooth_sidebar(state: &mut AppState) {
    let scale = state
        .smooth_sidebar()
        .then(|| state.sidebar_frame().draw_scale())
        .filter(|&scale| scale > 1);
    let side = current_sidebar_side(state);
    let (gpu, batch) = (&state.renderer.gpu, &state.renderer.batch_renderer);
    let presentation = &mut state.match_state.match_presentation;
    if let Some(atlas) = presentation.sidebar_cameo_atlas.as_mut() {
        atlas.set_smooth_scale(gpu, batch, scale);
    }
    if let Some(set) = presentation.sidebar_chrome.as_mut() {
        set.set_smooth_scale(side, gpu, batch, scale);
    }
}

/// `Sidebar.*` colours are `ColorStruct` triples; the sidebar text lane takes a
/// linear tint, which is also what the nullable harvester green falls back to.
#[cfg(feature = "phobos")]
fn sidebar_colour(rgb: [u8; 3]) -> [f32; 3] {
    rgb.map(|value| f32::from(value) / 255.0)
}

/// The hook's own base column for the harvester and weeds branches: `+50`.
#[cfg(feature = "phobos")]
const HARVESTER_ANCHOR_X: i32 = 50;
/// The hook's own base column for the power delta: `0x4A25E0` passes
/// `DSurface::Sidebar->GetWidth() / 2 - 70`, while the harvester and weeds
/// branches use `+ 50` and overlap each other by design.
#[cfg(feature = "phobos")]
const POWER_DELTA_ANCHOR_X: i32 = -70;
/// The literal `y = 2` row every indicator branch passes, the same anchor row
/// as the credits counter.
#[cfg(feature = "phobos")]
const INDICATOR_SURFACE_Y: i32 = 2;

/// The harvester-counter branch of the same hook (`Hooks.UI.cpp:111-135`),
/// reading `HouseExt::ActiveHarvesterCount` and `TotalHarvesterCount`
/// (`Ext/House/Body.cpp:278-303`).
///
/// The colour compares the *working-to-total* ratio with strict greater-than —
/// the reverse of the power delta's less-than — so a house with no counted
/// harvester at all reads `1.0` and stays green. `[label][working]/[total]`,
/// unless the side hides the total (`HideMaxValue`, which wins) or shows only
/// it (`OnlyMaxValue`). The branch opens on `HarvesterCounter.Show`, the
/// `[Phobos] ShowHarvesterCounter` client switch and a player who has not been
/// defeated; unlike the power delta it asks nothing about owned buildings.
#[cfg(feature = "phobos")]
fn harvester_counter_indicator(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    side: i32,
    owner: crate::sim::intern::InternedId,
    ui: &crate::rules::phobos_sidebar_indicators::SidebarIndicatorsUi,
    client_enabled: bool,
    tooltip_tint: [f32; 3],
    active: i32,
    total: i32,
) -> Option<crate::ui::sidebar::SidebarIndicator> {
    let counter = &ui.harvester;
    if !(counter.show && client_enabled) {
        return None;
    }
    if sim
        .houses()
        .get(&owner)
        .is_some_and(crate::sim::house_state::HouseState::is_defeated)
    {
        return None;
    }
    let percent = if total == 0 {
        1.0
    } else {
        f64::from(active) / f64::from(total)
    };
    let side_settings = rules.side_harvester_counter(side);
    let tint = if percent > counter.condition_yellow {
        // `ColorGreen` is a `Nullable`, whose source fallback is
        // `Drawing::TooltipColor` (`Hooks.UI.cpp:117`).
        side_settings
            .color_green
            .map(sidebar_colour)
            .unwrap_or(tooltip_tint)
    } else if percent > counter.condition_red {
        sidebar_colour(side_settings.color_yellow)
    } else {
        sidebar_colour(side_settings.color_red)
    };
    let text = if side_settings.hide_max_value {
        format!("{}{active}", counter.label)
    } else if side_settings.only_max_value {
        format!("{}{total}", counter.label)
    } else {
        format!("{}{active}/{total}", counter.label)
    };
    Some(crate::ui::sidebar::SidebarIndicator {
        text,
        tint,
        anchor_x: HARVESTER_ANCHOR_X + side_settings.offset[0],
        anchor_y: INDICATOR_SURFACE_Y + side_settings.offset[1],
        // The harvester and weeds branches hardcode TextPrintType::Center.
        align: crate::rules::phobos_sidebar_indicators::SidebarTextAlign::Center,
    })
}

/// The power-delta branch of Phobos's shared sidebar indicator hook
/// (`CreditsClass_GraphicLogic_HarvesterCounter @ 0x4A25E0`,
/// `src/Misc/Hooks.UI.cpp:137-173`).
///
/// Three conjuncts open it: `uimd.ini [Sidebar] PowerDelta.Show`, the
/// `[Phobos] ShowPowerDelta` client switch and a non-empty House+68; a defeated
/// player returns before the whole hook. The colour tracks the *drain-to-output
/// ratio*, never the displayed delta: `percent < ConditionYellow` is green,
/// Phobos' epsilon-tolerant `LESS_EQUAL(percent, ConditionRed)` — `(x - y) <=
/// 0.001` — is yellow, and zero output with any drain doubles the red threshold
/// so that branch lands on red. Zero output with zero drain keeps the yellow
/// fallback. A blackout (`HouseClass::PowerBlackoutTimer`, started by spy
/// infiltration at `House50BC90` and by the force shield) drops the number
/// entirely and draws `PowerBlackoutLabel` in `ColorGrey`.
#[cfg(feature = "phobos")]
fn power_delta_indicator(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    side: i32,
    owner: crate::sim::intern::InternedId,
    ui: &crate::rules::phobos_sidebar_indicators::SidebarIndicatorsUi,
    client_enabled: bool,
    power_produced: i32,
    power_drained: i32,
) -> Option<crate::ui::sidebar::SidebarIndicator> {
    let thresholds = ui.power_delta;
    let house = sim.houses().get(&owner)?;
    if !(thresholds.show && client_enabled)
        || house.is_defeated()
        || house.tracking().buildings() <= 0
    {
        return None;
    }
    let side_settings = rules.side_power_delta(side);
    let frame = sim.session().binary_frame();
    let blacked_out = sim
        .power_states()
        .get(&owner)
        .is_some_and(|power| power.blackout_remaining(frame) > 0);
    let (text, color) = if blacked_out {
        (ui.power_blackout_label.clone(), side_settings.color_grey)
    } else {
        let percent = if power_produced != 0 {
            f64::from(power_drained) / f64::from(power_produced)
        } else if power_drained != 0 {
            thresholds.condition_red * 2.0
        } else {
            thresholds.condition_yellow
        };
        let rgb = if percent < thresholds.condition_yellow {
            side_settings.color_green
        } else if percent - thresholds.condition_red <= 0.001 {
            side_settings.color_yellow
        } else {
            side_settings.color_red
        };
        // `%ls%+d`: the label then the signed net power, always with a sign.
        (
            format!(
                "{}{:+}",
                ui.power_label,
                power_produced.wrapping_sub(power_drained)
            ),
            rgb,
        )
    };
    Some(crate::ui::sidebar::SidebarIndicator {
        text,
        tint: sidebar_colour(color),
        anchor_x: POWER_DELTA_ANCHOR_X + side_settings.offset[0],
        anchor_y: INDICATOR_SURFACE_Y + side_settings.offset[1],
        align: side_settings.align,
    })
}

/// The weeds branch of the same hook (`src/Misc/Hooks.UI.cpp:175-190`), third
/// in native draw order after the harvester counter and the power delta.
///
/// Two conjuncts open it: `uimd.ini [Sidebar] WeedsCounter.Show` and the
/// `[Phobos] ShowWeedsCounter` client switch. Unlike the power delta it asks
/// nothing about owned buildings; the defeated player returns at the hook entry
/// (`:104`) before any branch, which VERA checks per branch as its siblings do.
/// It prints the bare total with no label (`L"%d"`), takes one nullable colour
/// falling back to the tooltip tint, and hardcodes `TextPrintType::Center`
/// rather than reading an alignment key.
#[cfg(feature = "phobos")]
fn weeds_counter_indicator(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    side: i32,
    owner: crate::sim::intern::InternedId,
    ui: &crate::rules::phobos_sidebar_indicators::SidebarIndicatorsUi,
    client_enabled: bool,
    tooltip_tint: [f32; 3],
) -> Option<crate::ui::sidebar::SidebarIndicator> {
    if !(ui.weeds.show && client_enabled) {
        return None;
    }
    let house = sim.houses().get(&owner)?;
    if house.is_defeated() {
        return None;
    }
    let side_settings = rules.side_weeds_counter(side);
    Some(crate::ui::sidebar::SidebarIndicator {
        text: format!("{}", house.economy().owned_weed_total()),
        tint: side_settings
            .color
            .map(sidebar_colour)
            .unwrap_or(tooltip_tint),
        anchor_x: HARVESTER_ANCHOR_X + side_settings.offset[0],
        anchor_y: INDICATOR_SURFACE_Y + side_settings.offset[1],
        align: crate::rules::phobos_sidebar_indicators::SidebarTextAlign::Center,
    })
}

/// `Ext/Sidebar/Hooks.cpp:44-77` (`SidebarClass_DrawIt_ProducingProgress
/// @ 0x6A6EB1`): the four strip columns, in native order — the building
/// factory (`BuildCat::DontCare`, so a combat-only house still fills i=0), the
/// combat factory, infantry, and the land-vehicle slot that falls through to
/// naval and then aircraft. The last chain asks for a factory *with* a held
/// object before falling through; `None` is native's `-1` (no factory, no
/// strip).
#[cfg(feature = "phobos")]
fn producing_progress_columns(
    sim: &crate::sim::world::Simulation,
    owner: crate::sim::intern::InternedId,
) -> [Option<u16>; 4] {
    use crate::sim::production::{PRODUCTION_STEPS, ProductionCategory};
    let progress = |category| {
        sim.production()
            .factories()
            .view(owner, category)
            .map(|view| view.progress.min(PRODUCTION_STEPS))
    };
    let held = |category| {
        sim.production()
            .factories()
            .view(owner, category)
            .filter(|view| view.object.is_some())
            .map(|view| view.progress.min(PRODUCTION_STEPS))
    };
    [
        progress(ProductionCategory::Building).or_else(|| progress(ProductionCategory::Defense)),
        progress(ProductionCategory::Defense),
        progress(ProductionCategory::Infantry),
        held(ProductionCategory::Vehicle)
            .or_else(|| held(ProductionCategory::Ship))
            .or_else(|| progress(ProductionCategory::Aircraft)),
    ]
}

/// `0x6A6EB1`: one strip per factory column at
/// `XBase + column * XOffset`, `YBase`, gated only by `uimd.ini
/// [Sidebar] ProducingProgress.Show`. Phobos blits `tab00pp.shp`..`tab03pp.shp`
/// frames; those ship with Phobos rather than retail, so VERA prints the same
/// completion as a percentage (`hud_boards::producing_progress_label`). The
/// text lane anchors on the surface midpoint, so the native absolute x is
/// rebased here.
#[cfg(feature = "phobos")]
fn producing_progress_indicators(
    ui: &crate::rules::phobos_sidebar_indicators::SidebarIndicatorsUi,
    side_settings: &crate::rules::phobos_sidebar_indicators::ProducingProgressSideSettings,
    progress: [Option<u16>; 4],
    gdi_positions: bool,
    surface_width: i32,
    tint: [f32; 3],
) -> Vec<crate::ui::sidebar::SidebarIndicator> {
    if !ui.producing_progress.show {
        return Vec::new();
    }
    progress
        .into_iter()
        .enumerate()
        .filter_map(|(column, progress)| {
            let progress = progress?;
            let (x, y) = super::hud_boards::producing_progress_anchor(
                side_settings.offset,
                gdi_positions,
                column,
            );
            Some(crate::ui::sidebar::SidebarIndicator {
                text: super::hud_boards::producing_progress_label(
                    progress,
                    crate::sim::production::PRODUCTION_STEPS,
                ),
                tint,
                anchor_x: x.wrapping_sub(surface_width / 2),
                anchor_y: y,
                align: crate::rules::phobos_sidebar_indicators::SidebarTextAlign::Left,
            })
        })
        .collect()
}

pub(crate) fn current_sidebar_theme(
    state: &AppState,
) -> crate::render::sidebar_chrome::SidebarTheme {
    crate::render::sidebar_chrome::sidebar_theme_for_side(current_sidebar_side(state))
}

/// The active native Scenario.Side owns Ares MIX selection. The retail sandbox
/// retains its development owner switch. Neither path rewrites House identity.
pub(crate) fn current_sidebar_side(state: &AppState) -> i32 {
    let sim = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation);
    #[cfg(feature = "ares")]
    let owner = None::<String>;
    #[cfg(not(feature = "ares"))]
    let owner = preferred_local_owner_name(state);
    project_sidebar_side(sim, owner.as_deref())
}

/// The production chrome and radar adapters share this side-index decision.
/// Original68781F/534FA0 and Ares1002FA00 consume the installed Scenario.Side;
/// the ordinary sandbox retains its existing live-owner development switch.
pub(crate) fn project_sidebar_side(
    simulation: Option<&crate::sim::world::Simulation>,
    owner: Option<&str>,
) -> i32 {
    #[cfg(feature = "ares")]
    if let Some(sim) = simulation {
        return sim.session().scenario_side();
    }
    owner
        .zip(simulation)
        .and_then(|(owner, sim)| {
            crate::sim::house_state::house_state_for_owner(sim.houses(), owner, sim.interner())
                .map(|house| house.side_index())
        })
        .unwrap_or(0)
}

fn current_sidebar_resolution(
    state: &AppState,
) -> Option<crate::render::sidebar_chrome::ResolvedSidebarChrome<'_>> {
    let set = state
        .match_state
        .match_presentation
        .sidebar_chrome
        .as_ref()?;
    set.resolve_side(current_sidebar_side(state))
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
        // No housing is left to settle and end the radar's static.
        crate::app::radar_movie::end_radar_sound_loop(state);
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
            &state.renderer.gpu.device,
            &state.renderer.gpu.queue,
            &state.renderer.batch_renderer,
            frames,
            width,
            height,
        ) {
            presentation.radar_anim = None;
        }
    } else {
        presentation.radar_anim = crate::render::radar::anim::RadarAnimState::new(
            &state.renderer.gpu.device,
            &state.renderer.gpu.queue,
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
        crate::app::radar_movie::end_radar_sound_loop(state);
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
        multiview_mask: None,
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
        multiview_mask: None,
    })
}

#[cfg(all(test, feature = "phobos"))]
#[path = "sidebar_indicator_tests.rs"]
mod sidebar_indicator_tests;

#[cfg(test)]
mod tests {
    use super::shows_alt_cameo;
    use crate::sim::house_state::HouseState;
    use crate::sim::intern::StringInterner;
    use crate::ui::sidebar::SidebarTab;
    use crate::ui::sidebar::cameo_order::{Cameo, CameoId, CameoKey, CameoStrips};
    use crate::ui::sidebar::gadget_flash::SidebarGadgetState;

    #[test]
    fn native_tab_initialization_disables_empty_strips() {
        let native = crate::test_fixture::json("tools/sidebar_oracle/geometry.json");
        for control in native["tab_initialization"].as_array().unwrap() {
            let mut active = SidebarTab::Building;
            let mut gadgets = SidebarGadgetState::default();
            for (index, before) in control["disabled_before"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
            {
                gadgets.set_tab_disabled(index, before.as_bool().unwrap());
            }
            let mut projection = crate::app::sidebar_projection::SidebarProjectionState::default();
            let reconciled = projection.reconcile_cameos(Default::default(), &[], 3, &[]);
            super::apply_cameo_transitions(reconciled.transitions(), &mut active, &mut gadgets);
            for (index, after) in control["disabled_after"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
            {
                assert_eq!(gadgets.tab_disabled[index], after.as_bool().unwrap());
                assert_eq!(
                    gadgets.tab_flashes[index].disabled,
                    after.as_bool().unwrap()
                );
                assert_eq!(gadgets.tab_frame(index, false), 2);
                assert_eq!(gadgets.tab_frame(index, true), 2);
            }
        }
    }

    #[test]
    fn restored_populated_tabs_replace_outgoing_disabled_state() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[Countries]\n0=Americans\n[Americans]\nSide=GDI\n\
                 [InfantryTypes]\n0=E1\n[E1]\nCost=100\nTechLevel=1\nOwner=Americans\n",
            ))
            .unwrap();
        let mut sim = crate::sim::world::Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.intern("Americans");
        sim.houses_mut_for_test()
            .register(HouseState::new(owner, 0, None, true, 1000, 10));
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        let infantry = sim.interner().get("E1").unwrap();
        let cameo = Cameo {
            id: CameoId::Object(infantry),
            tab: SidebarTab::Infantry,
            available: true,
            key: CameoKey::Techno {
                own_side: true,
                considered_aircraft: false,
                naval: false,
                internal_name: String::new(),
                sort_by_name: false,
                tech_level: 1,
                cost: 100,
                name: "E1".into(),
            },
        };
        let mut saved = crate::app::sidebar_projection::SidebarProjectionState::default();
        saved.reconcile_cameos(owner, &[cameo.clone()], 3, &[infantry]);
        let order = saved.saved_order().unwrap();
        let mut outgoing = crate::app::sidebar_projection::SidebarProjectionState::default();
        let mut active = SidebarTab::Building;
        let mut gadgets = SidebarGadgetState::default();
        gadgets.set_tab_disabled(SidebarTab::Infantry.tab_index(), true);
        let prepared =
            crate::app::sidebar_projection::prepare_saved_order(Some(order.clone()), &sim, &rules)
                .unwrap();
        outgoing.restore_order(prepared);
        let reconciled = outgoing.reconcile_cameos(owner, &[cameo], 3, &[]);
        super::apply_cameo_transitions(reconciled.transitions(), &mut active, &mut gadgets);
        assert_eq!(gadgets.tab_disabled, [true, true, false, true]);
        assert_eq!(outgoing.saved_order().unwrap().strips(), order.strips());
        assert!(
            !reconciled.build_inserted(),
            "saved entries are not new additions"
        );
    }

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
                internal_name: String::new(),
                sort_by_name: false,
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
            shows_alt_cameo(
                house,
                rules.object(kind).expect("a retail type"),
                rules,
                "",
                true,
            )
        };
        let mut barracks = HouseState::new(id, 0, None, true, 0, 10);
        barracks.grant_veteran_infantry();
        assert!(shows(&barracks, "E1"));
        assert!(!shows(&barracks, "SPY"), "Trainable=no");
        assert!(!shows(&barracks, "MTNK"));
        let mut war_factory = HouseState::new(id, 0, None, true, 0, 10);
        war_factory.grant_veteran_units();
        assert!(shows(&war_factory, "MTNK"));
        assert_eq!(
            shows(&war_factory, "DEST"),
            !crate::rules::ARES_ENABLED,
            "stock GetCameo uses the ground spy flag for ships; Ares separates naval gains"
        );
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

    #[cfg(feature = "ares")]
    #[test]
    fn ares_country_alternate_cameos_match_native_category_and_art_gates() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        let corpus = crate::test_fixture::json("tools/rules_oracle/ares_country_veterancy.json");
        for row in corpus["cameo_country"].as_array().unwrap() {
            if row["unit_list_self"].as_bool().unwrap() {
                // This native pointer-comparison witness places a BuildingType
                // in a UnitType-only vector, outside normal INI-produced state.
                assert!(row["input_limit"].as_str().is_some());
                continue;
            }
            let (registry, key) = match row["category"].as_str().unwrap() {
                "infantry" => ("InfantryTypes", "VeteranInfantry"),
                "unit" => ("VehicleTypes", "VeteranUnits"),
                "aircraft" => ("AircraftTypes", "VeteranAircraft"),
                "building" => ("BuildingTypes", "VeteranBuildings"),
                other => panic!("unknown native category {other}"),
            };
            let undeploys = row["undeploys"].as_bool().unwrap();
            let target_list = if row["unit_list_undeploy_target"].as_bool().unwrap() {
                "VeteranUnits=UNDEPLOY_TARGET\n"
            } else {
                ""
            };
            let registries = if registry == "VehicleTypes" {
                "[VehicleTypes]\n0=SUBJECT\n99=UNDEPLOY_TARGET\n".to_owned()
            } else {
                format!("[{registry}]\n0=SUBJECT\n[VehicleTypes]\n99=UNDEPLOY_TARGET\n")
            };
            let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
                "[Countries]\n0=SYNTHCOUNTRY\n[SYNTHCOUNTRY]\n{key}={}\n{target_list}\
                 {registries}\
                 [UNDEPLOY_TARGET]\nStrength=100\n[SUBJECT]\nStrength=100\nTrainable={}\n{}",
                if row["member"].as_bool().unwrap() {
                    "SUBJECT"
                } else {
                    "none"
                },
                if row["trainable"].as_bool().unwrap() {
                    "yes"
                } else {
                    "no"
                },
                if undeploys {
                    "UndeploysInto=UNDEPLOY_TARGET\n"
                } else {
                    ""
                },
            )))
            .unwrap();
            let mut interner = StringInterner::default();
            let owner = interner.intern("HOUSE");
            let country = interner.intern("SYNTHCOUNTRY");
            let house = HouseState::new(owner, 0, Some(country), true, 0, 10);
            assert_eq!(
                shows_alt_cameo(
                    &house,
                    rules.object("SUBJECT").unwrap(),
                    &rules,
                    "SYNTHCOUNTRY",
                    row["alt_art"].as_bool().unwrap()
                ),
                row["eligible"].as_bool().unwrap(),
                "{}",
                row["name"]
            );
        }
    }

    /// Rules for the two undeploying-building tests: a trainable building that
    /// undeploys into a unit the American country lists as veteran.
    #[cfg(feature = "ares")]
    fn undeploying_building_rules() -> crate::rules::ruleset::RuleSet {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        RuleSet::from_ini(&IniFile::from_str(
            "[Countries]\n0=Americans\n1=Russians\n\
             [Americans]\nVeteranUnits=AMCV\n[Russians]\n\
             [VehicleTypes]\n0=AMCV\n\
             [BuildingTypes]\n0=GACNST\n1=PLANT\n\
             [AMCV]\nStrength=200\nTrainable=yes\n\
             [GACNST]\nStrength=1000\nTrainable=yes\nUndeploysInto=AMCV\n\
             [PLANT]\nStrength=1000\nTrainable=yes\n",
        ))
        .unwrap()
    }

    /// Phobos `TechnoTypeExt::CameoIsVeteran`'s fixed building arm: a building
    /// that undeploys answers with its unit, so the war factory infiltration
    /// and the unit's own `VeteranUnits` entry both promote it.
    #[cfg(all(feature = "ares", feature = "phobos"))]
    #[test]
    fn a_deployable_building_reads_its_unit_for_the_alternate_cameo() {
        let rules = undeploying_building_rules();
        let owner = crate::sim::intern::InternedId::default();
        let building = rules.object("GACNST").expect("the deployable building");
        let mut house = HouseState::new(owner, 0, None, true, 0, 10);
        assert!(
            !shows_alt_cameo(&house, building, &rules, "Russians", true),
            "no infiltration and no list entry yet"
        );
        house.grant_veteran_units();
        assert!(
            shows_alt_cameo(&house, building, &rules, "Russians", true),
            "the war factory arm promotes the undeploying building"
        );

        let fresh = HouseState::new(owner, 0, None, true, 0, 10);
        assert!(
            shows_alt_cameo(&fresh, building, &rules, "Americans", true),
            "the unit's VeteranUnits entry reaches it through UndeploysInto"
        );
        let plant = rules.object("PLANT").expect("a plain building");
        assert!(
            !shows_alt_cameo(&fresh, plant, &rules, "Americans", true),
            "a building that does not undeploy keeps the Ares answer"
        );
    }

    /// Without Phobos the Ares arm stands: the building compares its own
    /// pointer against a unit-only list, which an INI list can never hold.
    #[cfg(all(feature = "ares", not(feature = "phobos")))]
    #[test]
    fn ares_alone_still_refuses_the_undeploying_buildings_alternate_cameo() {
        let rules = undeploying_building_rules();
        let owner = crate::sim::intern::InternedId::default();
        let building = rules.object("GACNST").expect("the deployable building");
        let mut house = HouseState::new(owner, 0, None, true, 0, 10);
        house.grant_veteran_units();
        assert!(
            !shows_alt_cameo(&house, building, &rules, "Americans", true),
            "Ares never reaches the unit list for an undeploying building"
        );
    }
}
