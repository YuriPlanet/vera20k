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

use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::input::commands::preferred_local_owner_name;
use crate::app::sidebar_projection::SidebarViewInputs;
use crate::sim::production;
use crate::ui::sidebar::{self, ArtKey, SidebarPart, SidebarView};

// Re-export instance builders so callers don't need to know about the split.
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
pub(crate) use crate::app::presentation::sidebar_build::{
    build_command_bar_instances, build_sidebar_cameo_instances, build_sidebar_chrome_instances,
};
use crate::app::presentation::state::MatchPresentationState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::ui::game_screen::GameScreen;

// ---------------------------------------------------------------------------
// Sidebar view construction
// ---------------------------------------------------------------------------

/// Return the one retained sidebar projection. Reading it never advances
/// credits, clears targeting, or clamps scroll state.
pub(crate) fn current_sidebar_view(presentation: &MatchPresentationState) -> Option<&SidebarView> {
    presentation.sidebar_projection.view()
}

/// Where the sidebar's parts sit: the retained view's layout, or, before a
/// view is built (or while the world has no local player), the layout the
/// view is built with, from the same table and frame.
pub(crate) fn current_sidebar_layout(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    presentation: &MatchPresentationState,
    persistence: &PersistenceState,
) -> sidebar::SidebarLayout {
    match current_sidebar_view(presentation) {
        Some(view) => view.layout,
        None => sidebar::compute_layout_with_spec(
            presentation.sidebar_layout_spec,
            crate::app::state::sidebar_frame(
                platform,
                persistence,
                renderer,
                screen,
                &presentation.sidebar_layout_spec,
            ),
        ),
    }
}

/// The money the credits counter counts toward: `CreditsClass::AI @
/// 0x004A2600` reads the player's Available_Money and shows a balance at or
/// below zero as 0 (`0x004A2687..0x004A2698`).
pub(crate) fn counter_credits(sim: &crate::sim::world::Simulation, owner: &str) -> i32 {
    production::credits_for_owner(sim, owner).max(0)
}

/// Step the credits counter once per Sidebar Action visit
/// ([`sidebar_action_visits`](crate::app::presentation::sidebar_gadgets::sidebar_action_visits)):
/// `CreditsClass::AI @ 0x004A2600`'s only caller is `TabClass::Action @
/// 0x006D099D`, which GScreen Input reaches. VERA keeps this clock running
/// with the world in a deactivated window, so a visible background match
/// keeps counting toward the balance production is changing. Each visit reads the balance the pass
/// ends with. Residual: Input runs before Logic (Main_Tick 0x0055D8AB), so
/// gamemd counts toward the balance one frame older; the counter starts
/// moving one frame later there, which a player cannot see.
pub(crate) fn advance_sidebar_credits(
    frontend: &FrontendState,
    match_state: &mut MatchState,
    visits: u64,
) {
    if visits == 0 {
        return;
    }
    let Some(owner_name) = preferred_local_owner_name(match_state) else {
        return;
    };
    let Some(sim) = match_state
        .sim_runtime
        .presentation()
        .map(|rt| &rt.simulation)
    else {
        return;
    };
    if sim.interner().get(&owner_name).is_some_and(|owner| {
        crate::app::presentation::observer_strip::shows_observer_sidebar(sim, owner)
    }) {
        // The observer's counter shows the time the match has run
        // (`0x004A2614..0x004A2678`): Scenario +0x61C plus the running span
        // since +0x614, over 60, set at once and silent.
        let seconds = match_state
            .scenario_elapsed_clock
            .elapsed_seconds(crate::app::match_runtime::frame_pacer::wall_clock_ms());
        match_state
            .match_presentation
            .sidebar_projection
            .model
            .set_observer_clock(&owner_name, seconds);
        return;
    }
    let credits = counter_credits(sim, &owner_name);
    let projection = &mut match_state.match_presentation.sidebar_projection;
    let mut last_step = None;
    for _ in 0..visits {
        let Some(step) = projection.model.advance_credits(&owner_name, credits) else {
            break;
        };
        last_step = Some(step);
    }
    let Some(tick) = last_step else {
        return;
    };
    // `CreditsClass::Draw @ 0x004A24F4..0x004A2533`: the `CreditTicks` cue
    // of the latest changed AI step — `[0]` counting up, `[1]` counting
    // down — at volume `0.5f`, centre pan, for the local player's counter
    // only (the observer branch draws elapsed time instead). Draw plays it
    // once per picture and clears the latch, so a pass plays at most one.
    let Some(sound_id) = crate::app::state::active_rules(frontend, match_state).and_then(|rules| {
        crate::app::sidebar_projection::credit_tick_sound(&rules.general.credit_ticks, tick)
            .map(|name| rules.sound_name(name))
    }) else {
        return;
    };
    match_state
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
pub(crate) fn refresh_sidebar_projection(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &PersistenceState,
) {
    refresh_projection(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
        false,
    );
}

/// The frame pass's republish on every unlocked presentation pass: the same
/// refresh, reusing the simulation inputs kept for the world's revision
/// ([`crate::app::sidebar_projection::SidebarSimInputs`]).
pub(crate) fn republish_sidebar_projection(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &PersistenceState,
) {
    refresh_projection(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
        true,
    );
}

/// The local side's sidebar table: the original's for its theme and position
/// set, with the loaded art's heights and whatever the side's `veraui.ini`
/// layout changes. Its width decides the battlefield's, so a new world takes
/// it before its first frame (`loading::transitions`).
pub(crate) fn sidebar_layout_spec(
    platform: &PlatformState,
    frontend: &FrontendState,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> crate::ui::sidebar::SidebarChromeLayoutSpec {
    let theme = current_sidebar_theme(frontend, match_state);
    // VERA-only: a map picture is taken with the original interface, whose
    // battlefield its tiles are cut for.
    let map_picture = platform.capture_presentation.map_picture;
    let mut spec = crate::ui::sidebar::SidebarChromeLayoutSpec::for_theme_and_positions(
        theme,
        gdi_positions(frontend, match_state),
    );
    if let Some(atlas) = current_sidebar_chrome(frontend, match_state) {
        let height = |key| {
            atlas
                .art(SidebarPart::Cameos, key, 0)
                .map(|art| art.pixel_size[1])
        };
        spec.side2_height = height(ArtKey::RowArt).unwrap_or(spec.side2_height);
        spec.side3_height = height(ArtKey::EndArt).unwrap_or(spec.side3_height);
    }
    // A mod's `veraui.ini` layout for this side changes the original's numbers.
    if let Some(layout) = side_layout(frontend, match_state).filter(|_| !map_picture) {
        layout.apply_to_sidebar(&mut spec);
    }
    // VERA's own: the player's cameo columns, laid inside the width the
    // layout leaves. After the layout, so the player's choice draws; a
    // capture keeps the original's two so its pictures stay reproducible.
    let columns = if platform.capture_client_size.is_some() {
        2
    } else {
        persistence.options_profile.sidebar_columns.count()
    };
    spec.apply_player_columns(columns);
    spec
}

/// The local player's side by its `[Sides]` name, as `[Layouts]` names it.
pub(crate) fn local_side_name<'a>(
    frontend: &'a FrontendState,
    match_state: &'a MatchState,
) -> Option<&'a str> {
    let rules = crate::app::state::active_rules(frontend, match_state)?;
    let side = u8::try_from(current_sidebar_side(frontend, match_state)).ok()?;
    rules.side_name(crate::rules::ruleset::SideIdx(side))
}

/// Whether the local side uses the allied position table: Phobos'
/// `Sidebar.GDIPositions=`, else whether its pictures are the Allies'.
fn gdi_positions(frontend: &FrontendState, match_state: &MatchState) -> bool {
    let allied = current_sidebar_theme(frontend, match_state)
        == crate::render::sidebar_chrome::SidebarTheme::Allied;
    #[cfg(feature = "phobos")]
    return crate::app::state::active_rules(frontend, match_state).map_or(allied, |rules| {
        rules.side_gdi_positions(current_sidebar_side(frontend, match_state))
    });
    #[cfg(not(feature = "phobos"))]
    allied
}

/// The `veraui.ini` layout of the local player's side, if the file gives it
/// one.
fn side_layout<'a>(
    frontend: &FrontendState,
    match_state: &'a MatchState,
) -> Option<&'a crate::ui::match_layout::MatchLayout> {
    match_state.match_presentation.match_layouts.for_side(
        local_side_name(frontend, match_state),
        gdi_positions(frontend, match_state),
    )
}

/// The grids of build pictures the local side's layout adds to the sidebar,
/// each where its part's place puts it and at the row the player scrolled it
/// to.
fn added_grids(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    presentation: &MatchPresentationState,
    persistence: &PersistenceState,
) -> Vec<sidebar::AddedGrid> {
    use crate::ui::added_parts::{AddedKind, Holder};
    let spec = presentation.sidebar_layout_spec;
    let side = crate::app::state::sidebar_frame(
        platform,
        persistence,
        renderer,
        screen,
        &presentation.sidebar_layout_spec,
    )
    .side_area();
    presentation
        .added_parts
        .iter()
        .enumerate()
        .filter_map(|(part, added)| {
            let AddedKind::CameoGrid { tab, columns, rows } = added.kind else {
                return None;
            };
            if added.holder != Holder::Side {
                return None;
            }
            let columns = columns.unwrap_or(spec.columns());
            // The grid is as big as its pictures' places.
            let natural = spec.grid_size(columns, rows).map(|v| v as i32);
            let [x, y, _, _] = added.rect_in(side, natural);
            Some(sidebar::AddedGrid {
                part,
                tab,
                origin: [x as f32, y as f32],
                columns,
                rows,
                scroll_rows: presentation
                    .added_grid_scroll
                    .get(part)
                    .copied()
                    .unwrap_or(0),
            })
        })
        .collect()
}

/// Keep the buttons and texts the local side's layout adds; none in a map
/// picture, which is taken with the original interface.
pub(crate) fn refresh_added_parts(
    platform: &PlatformState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
) {
    let map_picture = platform.capture_presentation.map_picture;
    let added = side_layout(frontend, match_state)
        .filter(|_| !map_picture)
        .map(|layout| layout.added().clone());
    let kept = &mut match_state.match_presentation.added_parts;
    match added {
        Some(added) if !std::sync::Arc::ptr_eq(&added, kept) => *kept = added,
        None if !kept.is_empty() => *kept = Default::default(),
        _ => {}
    }
    // What the layout gives the built-in parts, read again only when the
    // layout or the side's pictures change.
    use crate::app::presentation::layout_parts::StockParts;
    let theme = current_sidebar_theme(frontend, match_state);
    let original =
        crate::ui::match_layout::MatchLayout::original(gdi_positions(frontend, match_state));
    let kept = &match_state.match_presentation.stock_parts;
    let stock = match side_layout(frontend, match_state).filter(|_| !map_picture) {
        Some(layout) if kept.is_read_from(layout.added(), theme) => None,
        Some(layout) => Some(StockParts::read(layout, original, theme)),
        None if kept.is_original() => None,
        None => Some(StockParts::default()),
    };
    if let Some(stock) = stock {
        match_state.match_presentation.stock_parts = stock;
    }
}

fn refresh_projection(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &PersistenceState,
    reuse_sim_inputs: bool,
) {
    refresh_radar_animation_source(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
    );
    let spec = sidebar_layout_spec(platform, frontend, match_state, persistence);
    match_state.match_presentation.sidebar_layout_spec = spec;
    refresh_added_parts(platform, frontend, match_state);
    // The sidebar lays out on its own surface (`ui::sidebar::SidebarFrame`).
    let sidebar_frame = crate::app::state::sidebar_frame(
        platform,
        persistence,
        renderer,
        &frontend.screen,
        &match_state.match_presentation.sidebar_layout_spec,
    );
    let visible_rows = sidebar::compute_layout_with_spec(spec, sidebar_frame).side2_tile_count;
    let Some(owner_name) = preferred_local_owner_name(match_state) else {
        match_state.match_presentation.sidebar_projection.clear();
        return;
    };
    let Some(owner) = match_state.sim_runtime.presentation().map(|runtime| {
        runtime
            .simulation
            .interner()
            .get(&owner_name)
            .unwrap_or_default()
    }) else {
        match_state.match_presentation.sidebar_projection.clear();
        return;
    };
    // A scenario's first projection seeds its strips from every building's
    // construction options (`UpdateConstructionOptions`), once: a query of
    // the live world, which waits for a pass that holds it.
    let seed_additions = if match_state
        .match_presentation
        .sidebar_projection
        .model
        .needs_cameo_seed(owner)
    {
        if match_state.sim_runtime.is_lent() {
            return;
        }
        let Some(live) = match_state.sim_runtime.runtime() else {
            return;
        };
        production::initial_construction_options(&live.simulation, &live.resources.rules)
    } else {
        Vec::new()
    };
    // What the sidebar reads of the objects is the frame's
    // (`presentation::frame`), captured for the local player with the types
    // its strips hold and are about to add.
    let asked = match_state
        .match_presentation
        .sidebar_projection
        .model
        .asked_types(&seed_additions);
    match_state.sim_runtime.ask_sidebar_types(&asked);
    match_state.sim_runtime.ensure_frame();
    let inputs = {
        let Some(frame) = match_state.sim_runtime.frame() else {
            match_state.match_presentation.sidebar_projection.clear();
            return;
        };
        // Only a lent tick's frame can miss a type, one asked after the tick
        // left: the House pass waits for a frame that answers it.
        if !frame.scene.ui.sidebar_types().covers(&asked) {
            return;
        }
        let Some(inputs) = frame.scene.ui.sidebar() else {
            match_state.match_presentation.sidebar_projection.clear();
            return;
        };
        std::sync::Arc::clone(inputs)
    };

    let runtime = match_state
        .sim_runtime
        .presentation()
        .expect("sidebar runtime");
    let (sim, rules) = (&runtime.simulation, &runtime.resources.rules);
    // Phobos adds a superweapon to the left sidebar first and skips its right
    // sidebar cameo (`SidebarClass_AddCameo` returns false once accepted).
    #[cfg(feature = "phobos")]
    let sw_plan = crate::app::presentation::sw_sidebar::plan(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        persistence,
        owner,
        &inputs.sw_views,
    );
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
    let cameos = match_state
        .match_presentation
        .sidebar_projection
        .cameo_candidates(
            sim,
            rules,
            process_assets.csf.as_ref(),
            owner,
            &inputs,
            &right_sw_views,
            reuse_sim_inputs,
        );
    match_state
        .match_presentation
        .sidebar_projection
        .model
        .set_cameo_columns(spec.columns());
    let reconciled = match_state
        .match_presentation
        .sidebar_projection
        .model
        .reconcile_cameos(owner, &cameos, visible_rows, &seed_additions, rules.ares());
    match_state
        .match_presentation
        .sidebar_projection
        .model
        .set_observer_entries(inputs.observers.as_ref().map(Vec::len));
    match_state
        .match_presentation
        .sidebar_projection
        .model
        .set_cameo_frame(sim.session().binary_frame() as i32);
    match_state
        .match_presentation
        .sidebar_projection
        .model
        .reconcile_factory_links(sim, owner);
    match_state
        .match_presentation
        .sidebar_projection
        .model
        .advance_cameo_requests(sim, owner);
    {
        let presentation = &mut match_state.match_presentation;
        apply_cameo_transitions(
            reconciled.transitions(),
            &mut presentation.active_sidebar_tab,
            &mut presentation.sidebar_gadget_state,
        );
    }
    if reconciled.build_inserted() {
        crate::app::input::dispatch::push_local_eva(
            &mut match_state.match_audio,
            crate::app::input::sidebar_eva::EVA_NEW_CONSTRUCTION_OPTIONS,
        );
        crate::app::input::messages::post_eva_new_construction(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            &mut audio.sfx_player,
            persistence,
        );
    }

    let display_credits = match_state
        .match_presentation
        .sidebar_projection
        .model
        .displayed_credits_or_seed(&owner_name, inputs.credits);
    let (
        tab_btn_size,
        repair_btn_size,
        sell_btn_size,
        scroll_down_btn_size,
        scroll_up_btn_size,
        top_btn_sizes,
    ) = {
        let atlas = current_sidebar_chrome(frontend, match_state);
        let size = |part| {
            atlas
                .and_then(|atlas| atlas.art(part, ArtKey::Art, 0))
                .map(|entry| entry.pixel_size)
        };
        (
            size(SidebarPart::Buildings),
            size(SidebarPart::Repair),
            size(SidebarPart::Sell),
            size(SidebarPart::ScrollDown),
            size(SidebarPart::ScrollUp),
            [SidebarPart::Diplomacy, SidebarPart::Options].map(size),
        )
    };
    // Placement lifetime is driven by the input owner's ordered native
    // lifecycle facts. A missing Ready row alone is not pointer expiry:
    // Dispatch7258D0 can clear product while retaining type/foundation.
    // App targeting state -> the sidebar-owned armed projection (F06 seam).
    let armed_entry = match_state
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
        spec: match_state.match_presentation.sidebar_layout_spec,
        frame: sidebar_frame,
        tab: match_state.match_presentation.active_sidebar_tab,
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
        gadgets: match_state.match_presentation.sidebar_gadget_state.clone(),
        grids: added_grids(
            platform,
            renderer,
            &frontend.screen,
            &match_state.match_presentation,
            persistence,
        ),
    };
    // The gadget refresh runs this on every service pass, several per game
    // frame; while the inputs, strips and Factory links all hold, the view
    // it would build equals the retained one. Debug builds build it anyway
    // and compare.
    let holds = match_state
        .match_presentation
        .sidebar_projection
        .view_holds_for(&view_inputs);
    if !holds || crate::sim::live_read_check_enabled() {
        let view = build_view(frontend, match_state, process_assets, &view_inputs, owner);
        let projection = &mut match_state.match_presentation.sidebar_projection;
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
        let sw = &mut match_state.match_presentation.sw_sidebar;
        sw.layout = sw_plan.0;
        sw.entries = sw_plan.1;
    }
}

/// The view a refresh retains, built from `inputs` and the projection's
/// strips and Factory links, with each cameo's art and drawn state.
fn build_view(
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    inputs: &SidebarViewInputs,
    owner: crate::sim::intern::InternedId,
) -> SidebarView {
    let mut view = sidebar::build_sidebar_view_with_spec(
        inputs.spec,
        inputs.frame,
        inputs.tab,
        inputs.credits,
        inputs.sim.power_produced,
        inputs.sim.power_drained,
        inputs.button_sizes[0],
        &inputs.sim.queue_items,
        &inputs.sim.build_options,
        &inputs.sim.ready_buildings,
        inputs.armed.as_ref(),
        match_state
            .sim_runtime
            .presentation()
            .map(|rt| &rt.simulation)
            .map(|sim| sim.interner()),
        &inputs.supers,
        &inputs.gadgets,
        inputs.button_sizes[1],
        inputs.button_sizes[2],
        inputs.button_sizes[3],
        inputs.button_sizes[4],
        [inputs.button_sizes[5], inputs.button_sizes[6]],
        match_state
            .match_presentation
            .sidebar_projection
            .model
            .cameo_strips(),
    );
    // The grids the side's layout adds follow the sidebar's own in the view,
    // and each keeps the row it could really scroll to.
    sidebar::append_added_grids(
        &mut view,
        inputs.spec,
        &inputs.grids,
        &inputs.sim.queue_items,
        &inputs.sim.build_options,
        &inputs.sim.ready_buildings,
        inputs.armed.as_ref(),
        match_state
            .sim_runtime
            .presentation()
            .map(|rt| &rt.simulation)
            .map(|sim| sim.interner()),
        &inputs.supers,
        match_state
            .match_presentation
            .sidebar_projection
            .model
            .cameo_strips(),
    );
    {
        let scroll = &mut match_state.match_presentation.added_grid_scroll;
        for grid in &view.added_grids {
            if scroll.len() <= grid.part {
                scroll.resize(grid.part + 1, 0);
            }
            scroll[grid.part] = grid.scroll_rows;
        }
    }
    // While the local player observes, the strip lists the players over the
    // grid, and its TopRow counts them.
    if let Some(houses) = inputs.sim.observers.as_deref()
        && let Some(sim) = match_state
            .sim_runtime
            .presentation()
            .map(|rt| &rt.simulation)
    {
        let entries = crate::app::presentation::observer_strip::observer_entries(
            frontend,
            match_state,
            process_assets,
            sim,
            houses,
        );
        let top = match_state
            .match_presentation
            .sidebar_projection
            .model
            .cameo_strips()
            .scroll_row(inputs.tab);
        sidebar::apply_observer_strip(&mut view, inputs.spec, entries, top, &inputs.gadgets);
    }
    // The view never substitutes another tab, so the retained active tab needs
    // no write-back: `apply_cameo_transitions` (and the switch itself) is the
    // only writer.
    match_state
        .match_presentation
        .sidebar_projection
        .model
        .set_scroll_row(inputs.tab, view.scroll_rows);
    if let Some(atlas) = match_state.match_presentation.sidebar_cameo_atlas.as_ref() {
        let simulation = match_state
            .sim_runtime
            .presentation()
            .map(|rt| &rt.simulation);
        let house = simulation.and_then(|sim| sim.houses().get(&owner));
        let rules = crate::app::state::active_rules(frontend, match_state);
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
    if let (Some(runtime), Some(frame)) = (
        match_state.sim_runtime.presentation(),
        match_state.sim_runtime.frame(),
    ) {
        let sim = &runtime.simulation;
        let armed_building = inputs
            .armed
            .as_ref()
            .and_then(sidebar::ArmedSidebarEntry::as_building_placement)
            .and_then(|name| sim.interner().get(name));
        match_state
            .match_presentation
            .sidebar_projection
            .model
            .apply_cameo_draw_state(
                &mut view,
                sim,
                frame,
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
pub(crate) fn refresh_sidebar_cameo_draw_state(match_state: &mut MatchState) {
    let Some(owner_name) = preferred_local_owner_name(match_state) else {
        return;
    };
    // The entries' availability is the frame's (`presentation::frame`).
    match_state.sim_runtime.ensure_frame();
    let (Some(runtime), Some(frame)) = (
        match_state.sim_runtime.presentation(),
        match_state.sim_runtime.frame(),
    ) else {
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
        .refresh_cameo_draw_state(sim, frame, &runtime.resources.rules, owner, armed_building);
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
/// With Ares, TechnoTypeClass_GetCameo (hook `0x712045`, Ares.dll
/// `1005E480`) replaces the choice with CameoIsElite (`1003D800`), which
/// additionally admits the current Country's retained lists.
/// An undeploying Building uses the Unit-list branch, comparing its own
/// BuildingType pointer, not UndeploysInto; that cannot match an INI-produced
/// UnitType list. Its spy arms read HouseState's stock flags and the Ares
/// SpyEffect veterancy (`sim::spy_infiltration`).
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
    if rules.ares() {
        if obj.category == ObjectCategory::Building && obj.undeploys_into.is_some() {
            return false;
        }
        if rules.country_starts_veteran(country, obj) {
            return true;
        }
        // Ares 3.0 1003D89A/1003D8DE/1003D911: a naval unit asks the
        // house's SpyEffect NavalVeterancy instead of the war factory's,
        // aircraft and non-undeploying buildings their own.
        return obj.trainable
            && match obj.category {
                ObjectCategory::Infantry => !obj.naval && house.veteran_infantry(),
                ObjectCategory::Vehicle if obj.naval => house.naval_veterancy(),
                ObjectCategory::Vehicle => house.veteran_units(),
                ObjectCategory::Aircraft => house.aircraft_veterancy(),
                ObjectCategory::Building => house.building_veterancy(),
            };
    }
    obj.trainable
        && match obj.category {
            ObjectCategory::Infantry => house.veteran_infantry(),
            ObjectCategory::Vehicle => house.veteran_units(),
            ObjectCategory::Building => house.veteran_units() && obj.undeploys_into.is_some(),
            ObjectCategory::Aircraft => false,
        }
}

/// Whether the minimap is drawn and takes clicks: the layout has the radar,
/// and the radar is online (or has no housing animation, the legacy
/// fallback).
pub(crate) fn minimap_shown(presentation: &MatchPresentationState) -> bool {
    presentation
        .sidebar_layout_spec
        .shows(crate::ui::sidebar::SidebarPart::Radar)
        && presentation
            .radar_anim
            .as_ref()
            .is_none_or(|ra| ra.is_minimap_visible())
}

/// Whether the cursor is over the radar's map gadget (`0x00B04A10`): the
/// aperture, online or not. [`is_cursor_over_minimap`] is its image.
pub(crate) fn is_cursor_over_radar_gadget(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> bool {
    let (x, y) = sidebar_cursor(
        platform,
        renderer,
        &frontend.screen,
        match_state,
        persistence,
    );
    match_state
        .match_presentation
        .sidebar_layout_spec
        .shows(crate::ui::sidebar::SidebarPart::Radar)
        && active_minimap_screen_rect(platform, renderer, frontend, match_state, persistence)
            .contains(x, y)
}

pub(crate) fn is_cursor_over_minimap(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> bool {
    // Minimap interaction disabled when radar is not online.
    if !minimap_shown(&match_state.match_presentation) {
        return false;
    }
    let Some(minimap) = &match_state.match_presentation.minimap else {
        return false;
    };
    let rect = active_minimap_screen_rect(platform, renderer, frontend, match_state, persistence);
    let (x, y) = sidebar_cursor(
        platform,
        renderer,
        &frontend.screen,
        match_state,
        persistence,
    );
    minimap.contains_screen_point_in_rect(x, y, rect.x, rect.y, rect.w, rect.h)
}

pub(crate) fn minimap_camera_target(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> Option<crate::render::radar::NativeRadarCameraTarget> {
    let rect = active_minimap_screen_rect(platform, renderer, frontend, match_state, persistence);
    let (cursor_x, cursor_y) = sidebar_cursor(
        platform,
        renderer,
        &frontend.screen,
        match_state,
        persistence,
    );
    let minimap = match_state.match_presentation.minimap.as_ref()?;
    // Input may run before any render: the frame of the current world.
    let frame = match_state.sim_runtime.frame()?;
    let runtime = match_state.sim_runtime.presentation();
    let native_facts = runtime.and_then(|runtime| {
        let view = runtime.view();
        Some((
            view.resolved_terrain()?,
            view.simulation().map_size_diamond()?,
        ))
    });
    let (tactical_w, tactical_h) = crate::app::input::camera::tactical_viewport_size(
        platform,
        renderer,
        &frontend.screen,
        &match_state.match_presentation,
        persistence,
    );
    native_facts.and_then(|(terrain, map_size)| {
        minimap.native_click_target_in_rect(
            cursor_x,
            cursor_y,
            sidebar_cursor_centre(
                platform,
                renderer,
                &frontend.screen,
                match_state,
                persistence,
            ),
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            frame,
            terrain,
            map_size,
            (tactical_w as i32, tactical_h as i32),
        )
    })
}

/// The order/cursor target precedes the camera branch's viewport clamp in
/// RadarClass::Input (`0x00653ABC` versus `0x00653D7A`).
pub(crate) fn minimap_input_target(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> Option<crate::render::radar::NativeRadarInputTarget> {
    let rect = active_minimap_screen_rect(platform, renderer, frontend, match_state, persistence);
    // Input may run before any render: the frame of the current world.
    let frame = match_state.sim_runtime.frame()?;
    match_state
        .match_presentation
        .minimap
        .as_ref()?
        .native_input_target_in_rect(
            sidebar_cursor(
                platform,
                renderer,
                &frontend.screen,
                match_state,
                persistence,
            ),
            sidebar_cursor_centre(
                platform,
                renderer,
                &frontend.screen,
                match_state,
                persistence,
            ),
            [rect.x, rect.y, rect.w, rect.h],
            frame,
        )
}

pub(crate) fn minimap_object_click_position(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    persistence: &PersistenceState,
    id: u64,
) -> Option<[u32; 2]> {
    let rect = active_minimap_screen_rect(platform, renderer, frontend, match_state, persistence);
    let minimap = match_state.match_presentation.minimap.as_ref()?;
    let (x, y) = minimap.native_object_click_point_in_rect(id, [rect.x, rect.y, rect.w, rect.h])?;
    let [x, y, ..] = crate::app::state::sidebar_frame(
        platform,
        persistence,
        renderer,
        &frontend.screen,
        &match_state.match_presentation.sidebar_layout_spec,
    )
    .rect_to_frame_px(crate::ui::sidebar::Rect {
        x,
        y,
        w: 1.0,
        h: 1.0,
    });
    Some([x.max(0) as u32, y.max(0) as u32])
}

pub(crate) fn update_camera_from_minimap_cursor(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    persistence: &PersistenceState,
) {
    let native_target =
        minimap_camera_target(platform, renderer, frontend, match_state, persistence);
    if let Some(target) = native_target {
        // `0x00653EA0 -> FUN_006D6070` writes current and desired viewport
        // together from the complete CellClass center XYZ. The camera owner
        // discards any pending scroll request when setting the absolute view.
        let (x, y, z) = target.world_leptons;
        crate::app::input::camera::center_camera_on_lepton_point(
            platform,
            renderer,
            &frontend.screen,
            match_state,
            persistence,
            x,
            y,
            z,
        );
        return;
    }
    let rect = active_minimap_screen_rect(platform, renderer, frontend, match_state, persistence);
    let Some(minimap) = match_state.match_presentation.minimap.as_ref() else {
        return;
    };
    if minimap.has_playfield_authority() {
        // A live map whose generated surface is unavailable must not silently
        // approximate the click through the old normalized 200x200 transform.
        return;
    }

    // Mapless/headless presentation fixture adapter only.
    let (cursor_x, cursor_y) = sidebar_cursor(
        platform,
        renderer,
        &frontend.screen,
        match_state,
        persistence,
    );
    let sw = renderer.render_width(&frontend.screen) as f32;
    let sh = renderer.render_height(&frontend.screen) as f32;
    // A view centre becomes a camera position through the centring size.
    let (tactical_w, tactical_h) = crate::app::input::camera::tactical_centring_size(
        platform,
        renderer,
        &frontend.screen,
        &match_state.match_presentation,
        persistence,
    );
    let z = match_state.input.zoom_level;
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
    crate::app::input::camera::set_camera_position(&mut match_state.input, (cx, cy));
    crate::app::input::camera::clamp_camera_to_playable_area(
        platform,
        renderer,
        match_state,
        persistence,
        sw,
        sh,
    );
}

/// Live generated-primary screen rectangle used by the retained input region.
/// Centered letterbox margins are not part of RadarClass's click surface.
pub(crate) fn active_minimap_content_screen_rect(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> crate::ui::sidebar::Rect {
    let aperture =
        active_minimap_screen_rect(platform, renderer, frontend, match_state, persistence);
    let Some(minimap) = match_state.match_presentation.minimap.as_ref() else {
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
fn sidebar_cursor(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> (f32, f32) {
    crate::app::state::sidebar_frame(
        platform,
        persistence,
        renderer,
        screen,
        &match_state.match_presentation.sidebar_layout_spec,
    )
    .point_to_layout(match_state.input.cursor_x, match_state.input.cursor_y)
}

/// The centre of the window pixel under the cursor, in the same layout space.
/// The cursor sits on that pixel's corner in frame pixels; the HD radar picks
/// its own finer pixels at the centre.
fn sidebar_cursor_centre(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> (f32, f32) {
    let half = 0.5
        / renderer
            .match_zoom_pass(screen)
            .map_or(1.0, |pass| pass.geometry().scale());
    crate::app::state::sidebar_frame(
        platform,
        persistence,
        renderer,
        screen,
        &match_state.match_presentation.sidebar_layout_spec,
    )
    .point_to_layout(
        match_state.input.cursor_x + half,
        match_state.input.cursor_y + half,
    )
}

/// The radar aperture in the sidebar's layout space. Input regions take it in
/// frame pixels through [`crate::ui::sidebar::SidebarFrame::rect_to_frame`].
pub(crate) fn active_minimap_screen_rect(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> crate::ui::sidebar::Rect {
    if current_sidebar_chrome(frontend, match_state).is_some() {
        current_sidebar_layout(
            platform,
            renderer,
            &frontend.screen,
            &match_state.match_presentation,
            persistence,
        )
        .minimap_rect(&match_state.match_presentation.sidebar_layout_spec)
    } else {
        // Archives without any sidebar art (`build_sidebar_chrome_set`'s
        // `None`) still draw the map, in the battlefield's corner.
        let (_, sh) = crate::app::state::sidebar_frame(
            platform,
            persistence,
            renderer,
            &frontend.screen,
            &match_state.match_presentation.sidebar_layout_spec,
        )
        .layout_screen();
        let (x, y, w, h) = crate::render::radar::minimap::default_minimap_rect(sh);
        crate::ui::sidebar::Rect { x, y, w, h }
    }
}

// ---------------------------------------------------------------------------
// Chrome / theme helpers
// ---------------------------------------------------------------------------

/// Build the sidebar chrome of the side the local player draws this frame
/// when the set lacks it (`SidebarChromeSet::ensure_side`): a stock match
/// builds the local side's at load, and a change of local house asks here.
pub(crate) fn ensure_current_sidebar_chrome(
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
) {
    let side = current_sidebar_side(frontend, match_state);
    let Some(assets) = process_assets.manager() else {
        return;
    };
    if let Some(set) = match_state.match_presentation.sidebar_chrome.as_mut() {
        set.ensure_side(side, &renderer.gpu, &renderer.batch_renderer, assets);
    }
}

/// Keep VERA's smooth sidebar (`render::sidebar_smoothing`) at this frame's
/// draw scale, or drop its copies when the option is off, the sidebar draws
/// at its own pixels, or the scale is not a whole one (xBR has no 1.5x).
/// HD art (`render::hd_switch`) turns it on too: its HD pictures come through it.
pub(crate) fn prepare_smooth_sidebar(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    persistence: &PersistenceState,
) {
    let scale = (persistence.smooth_sidebar(platform.capture_choices())
        || crate::app::state::hd_art_on(persistence).0)
        .then(|| {
            crate::app::state::sidebar_frame(
                platform,
                persistence,
                renderer,
                &frontend.screen,
                &match_state.match_presentation.sidebar_layout_spec,
            )
            .draw_scale()
        })
        .filter(|scale| *scale >= 2.0 && scale.fract() == 0.0)
        .map(|scale| scale as u32);
    let side = current_sidebar_side(frontend, match_state);
    let (gpu, batch) = (&renderer.gpu, &renderer.batch_renderer);
    let presentation = &mut match_state.match_presentation;
    if let Some(atlas) = presentation.sidebar_cameo_atlas.as_mut() {
        atlas.set_smooth_scale(gpu, batch, scale);
    }
    if let Some(set) = presentation.sidebar_chrome.as_mut() {
        set.set_smooth_scale(side, gpu, batch, scale);
    }
}

/// The theme follows the arm the chrome set was built by, so it names the art
/// the sidebar draws: a side without a retained Side sidebar configuration
/// draws the stock sidebar. Before a set exists, the game's Ares switch
/// (`RuleSet::ares`) decides.
pub(crate) fn current_sidebar_theme(
    frontend: &FrontendState,
    match_state: &MatchState,
) -> crate::render::sidebar_chrome::SidebarTheme {
    let ares = match match_state.match_presentation.sidebar_chrome.as_ref() {
        Some(set) => set.ares(),
        None => {
            crate::app::state::active_rules(frontend, match_state).is_some_and(|rules| rules.ares())
        }
    };
    crate::render::sidebar_chrome::sidebar_theme_for_side(
        current_sidebar_side(frontend, match_state),
        ares,
    )
}

/// When Ares plays in the game (`RuleSet::ares`), the active native
/// Scenario.Side owns Ares MIX selection. Otherwise the retail sandbox retains
/// its development owner switch. Neither path rewrites House identity.
pub(crate) fn current_sidebar_side(frontend: &FrontendState, match_state: &MatchState) -> i32 {
    let ares =
        crate::app::state::active_rules(frontend, match_state).is_some_and(|rules| rules.ares());
    let sim = match_state
        .sim_runtime
        .presentation()
        .map(|rt| &rt.simulation);
    let owner = if ares {
        None
    } else {
        preferred_local_owner_name(match_state)
    };
    project_sidebar_side(sim, owner.as_deref(), ares)
}

/// The production chrome and radar adapters share this side-index decision.
/// Original68781F/534FA0 and Ares1002FA00 consume the installed Scenario.Side
/// when Ares plays in the game (`ares`, `RuleSet::ares`); the ordinary sandbox
/// retains its existing live-owner development switch.
pub(crate) fn project_sidebar_side(
    simulation: Option<&crate::sim::world::Simulation>,
    owner: Option<&str>,
    ares: bool,
) -> i32 {
    if ares && let Some(sim) = simulation {
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

fn current_sidebar_resolution<'a>(
    frontend: &FrontendState,
    match_state: &'a MatchState,
) -> Option<crate::render::sidebar_chrome::ResolvedSidebarChrome<'a>> {
    let set = match_state.match_presentation.sidebar_chrome.as_ref()?;
    set.resolve_side(current_sidebar_side(frontend, match_state))
}

pub(crate) fn current_sidebar_chrome<'a>(
    frontend: &FrontendState,
    match_state: &'a MatchState,
) -> Option<&'a crate::render::sidebar_chrome::SidebarChromeAtlas> {
    current_sidebar_resolution(frontend, match_state).map(|resolved| resolved.atlas)
}

/// 652E90 installs radar shape/palette from the current scenario side, without
/// resetting frame/time. VERA's unpinned sandbox owner switch uses a forced
/// source redraw; this development interaction has no native lifecycle claim.
fn refresh_radar_animation_source(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &PersistenceState,
) {
    let Some(resolved) = current_sidebar_resolution(frontend, match_state) else {
        let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        crate::app::radar_movie::retire(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            wall_ms,
        );
        // No housing is left to settle and end the radar's static.
        crate::app::radar_movie::end_radar_sound_loop(audio);
        let presentation = &mut match_state.match_presentation;
        presentation.radar_anim = None;
        presentation.radar_animation_source = None;
        presentation.radar_content_insets = None;
        return;
    };
    if match_state
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
    let presentation = &mut match_state.match_presentation;
    if let Some(radar) = presentation.radar_anim.as_mut() {
        if !radar.replace_frames(
            &renderer.gpu.device,
            &renderer.gpu.queue,
            &renderer.batch_renderer,
            frames,
            width,
            height,
        ) {
            presentation.radar_anim = None;
        }
    } else {
        presentation.radar_anim = crate::render::radar::anim::RadarAnimState::new(
            &renderer.gpu.device,
            &renderer.gpu.queue,
            &renderer.batch_renderer,
            frames,
            width,
            height,
        );
        if let Some(radar) = presentation.radar_anim.as_mut() {
            //652E90 source installation does not request a new availability
            //edge. A new scenario arrives with has_radar clear and opens on
            //its first House update; a loaded one arrives with its saved
            //housing, which MouseLoad5BE092 restores silent.
            match presentation.loaded_radar_housing.take() {
                Some(saved) => radar.restore_saved_housing(saved),
                None => radar.restore_availability(presentation.has_radar),
            }
        }
    }
    presentation.radar_animation_source = Some(identity);
    presentation.radar_content_insets = Some(insets);
    if presentation.radar_anim.is_none() {
        let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        crate::app::radar_movie::retire(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            wall_ms,
        );
        crate::app::radar_movie::end_radar_sound_loop(audio);
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

#[cfg(all(test, tests_app_presentation))]
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
            let reconciled =
                projection
                    .model
                    .reconcile_cameos(Default::default(), &[], 3, &[], false);
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
                tech_level: 1,
                cost: 100,
                name: "E1".into(),
            },
        };
        let mut saved = crate::app::sidebar_projection::SidebarProjectionState::default();
        saved
            .model
            .reconcile_cameos(owner, &[cameo.clone()], 3, &[infantry], false);
        let order = saved.model.saved_order().unwrap();
        let mut outgoing = crate::app::sidebar_projection::SidebarProjectionState::default();
        let mut active = SidebarTab::Building;
        let mut gadgets = SidebarGadgetState::default();
        gadgets.set_tab_disabled(SidebarTab::Infantry.tab_index(), true);
        let prepared =
            crate::app::sidebar_projection::prepare_saved_order(Some(order.clone()), &sim, &rules)
                .unwrap();
        outgoing.model.restore_order(prepared);
        let reconciled = outgoing
            .model
            .reconcile_cameos(owner, &[cameo], 3, &[], false);
        super::apply_cameo_transitions(reconciled.transitions(), &mut active, &mut gadgets);
        assert_eq!(gadgets.tab_disabled, [true, true, false, true]);
        assert_eq!(
            outgoing.model.saved_order().unwrap().strips(),
            order.strips()
        );
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
            let reconciled = strips.reconcile_fixture(candidates, 3, true, false);
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
            !rules.ares(),
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

    #[test]
    fn ares_country_alternate_cameos_match_native_category_and_art_gates() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        let _ares = crate::rules::ares_switch::read_ares_way(true);
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
}

/// What the sidebar, the house UI and the sounds read of the world's
/// objects, captured into the presentation frame
/// (`presentation::frame_scene::SceneFrame`) for its local player.
#[derive(Clone, Debug, Default)]
pub(crate) struct UiFrame {
    /// `Simulation::theoretical_power_for_owner` of the local player, the
    /// power bar's fit.
    theoretical_power: i32,
    /// The local player's sidebar inputs
    /// ([`crate::app::sidebar_projection::SidebarSimInputs::derive`]).
    sidebar: Option<std::sync::Arc<crate::app::sidebar_projection::SidebarSimInputs>>,
    /// The object types those inputs answer: what the sidebar had asked when
    /// the frame was captured (`SimHost::ask_sidebar_types`).
    sidebar_types: crate::app::sidebar_projection::SidebarTypes,
    /// `production::cameo_draw_state` of each of those build options' types:
    /// what `StripClass::Draw` asks of every entry it draws.
    cameo_draw: std::collections::HashMap<
        crate::sim::intern::InternedId,
        crate::sim::production::CameoDrawState,
    >,
    /// Each live bomb's ticking loop, in carrier order
    /// (`Simulation::bomb_carriers`).
    bomb_ticks: Vec<BombTick>,
    /// `Simulation::looping_sound_owner_coord` of each ticking-bomb owner and
    /// each object that is not a Unit, Infantry or Aircraft: the owners whose
    /// loops the frame tail re-drives from their coordinate.
    sound_owner_coords: std::collections::HashMap<u64, crate::sim::anim_class::AnimWorldCoord>,
    /// The terrain objects that stand for their own sound handles
    /// (`Simulation::object_sound_owner_alive`); an object's answer is its
    /// record's ([`crate::app::presentation::frame::PresentationFrame::sound_object_stands`]).
    standing_terrain_objects: std::collections::HashSet<u64>,
}

/// One live bomb's ticking loop (`BombListClass::UpdateAll @ 0x00438BF0`'s
/// ticking pass).
#[derive(Clone, Debug)]
pub(crate) struct BombTick {
    /// Its loop's owner (`sim::bomb::ticking_sound_owner`).
    pub(crate) owner: u64,
    /// `Simulation::bomb_ticking_sound`.
    pub(crate) sound: Option<String>,
    /// `Simulation::bomb_ticking_coord`.
    pub(crate) coord: Option<crate::sim::anim_class::AnimWorldCoord>,
}

impl UiFrame {
    pub(crate) fn capture(at: &crate::app::presentation::frame::Capture<'_>) -> Self {
        use crate::map::entities::EntityCategory::{Aircraft, Infantry, Unit};
        let (sim, rules) = (at.sim, at.rules());
        let sidebar = at.observer.map(|owner| {
            std::sync::Arc::new(crate::app::sidebar_projection::SidebarSimInputs::derive(
                sim,
                rules,
                owner,
                at.sidebar_types,
            ))
        });
        let cameo_draw = match (&sidebar, at.observer_id) {
            (Some(inputs), Some(owner)) => inputs
                .build_options
                .iter()
                .filter_map(|option| {
                    let object = rules.object(sim.interner().resolve(option.type_id))?;
                    Some((
                        option.type_id,
                        production::cameo_draw_state(sim, rules, owner, object),
                    ))
                })
                .collect(),
            _ => Default::default(),
        };
        let bomb_ticks: Vec<BombTick> = sim
            .bomb_carriers()
            .iter()
            .map(|&carrier| BombTick {
                owner: crate::sim::bomb::ticking_sound_owner(carrier),
                sound: sim.bomb_ticking_sound(carrier).map(str::to_owned),
                coord: sim.bomb_ticking_coord(carrier),
            })
            .collect();
        // The objects' loop coordinates, asked in parallel.
        let object_coords = sim.entities().par_filter_map(|id, entity| {
            if matches!(entity.category(), Unit | Infantry | Aircraft) {
                return None;
            }
            Some((id, sim.looping_sound_owner_coord(id)?))
        });
        let sound_owner_coords = bomb_ticks
            .iter()
            .filter_map(|tick| Some((tick.owner, sim.looping_sound_owner_coord(tick.owner)?)))
            .chain(object_coords)
            .collect();
        let standing_terrain_objects = sim
            .terrain_objects()
            .objects()
            .keys()
            .copied()
            .filter(|&object| {
                sim.object_sound_owner_alive(crate::sim::world::ambient_sound_owner(object))
                    == Some(true)
            })
            .collect();
        Self {
            theoretical_power: at
                .observer
                .map_or(0, |owner| sim.theoretical_power_for_owner(rules, owner)),
            sidebar,
            sidebar_types: at.sidebar_types.clone(),
            cameo_draw,
            bomb_ticks,
            sound_owner_coords,
            standing_terrain_objects,
        }
    }

    /// `Simulation::theoretical_power_for_owner` of the local player.
    pub(crate) fn theoretical_power(&self) -> i32 {
        self.theoretical_power
    }

    /// The local player's sidebar inputs; `None` without a local player.
    pub(crate) fn sidebar(
        &self,
    ) -> Option<&std::sync::Arc<crate::app::sidebar_projection::SidebarSimInputs>> {
        self.sidebar.as_ref()
    }

    /// The object types [`Self::sidebar`] and [`Self::cameo_draw_state`]
    /// answer.
    pub(crate) fn sidebar_types(&self) -> &crate::app::sidebar_projection::SidebarTypes {
        &self.sidebar_types
    }

    /// `production::cameo_draw_state` of the local player's type `type_id`;
    /// unavailable for a type no rules list builds or the sidebar has not
    /// asked.
    pub(crate) fn cameo_draw_state(
        &self,
        type_id: crate::sim::intern::InternedId,
    ) -> crate::sim::production::CameoDrawState {
        self.cameo_draw
            .get(&type_id)
            .copied()
            .unwrap_or(crate::sim::production::CameoDrawState {
                enabled: false,
                at_build_limit: false,
            })
    }

    pub(crate) fn bomb_ticks(&self) -> &[BombTick] {
        &self.bomb_ticks
    }

    /// `Simulation::looping_sound_owner_coord` of `owner`, for a bomb's or an
    /// object's loop; an Anim's is the frame's
    /// ([`crate::app::presentation::frame::PresentationFrame::anim_coord`]).
    pub(crate) fn sound_owner_coord(
        &self,
        owner: u64,
    ) -> Option<crate::sim::anim_class::AnimWorldCoord> {
        self.sound_owner_coords.get(&owner).copied()
    }

    /// Whether terrain object `object` stands for its own sound handles
    /// (`Simulation::object_sound_owner_alive`).
    pub(crate) fn terrain_object_stands(&self, object: u64) -> bool {
        self.standing_terrain_objects.contains(&object)
    }
}
