//! Successful saved-world adoption and its route-specific presentation state.

use crate::app::AppState;

/// Apply the validated world/resources and the already-prepared asset bundle.
/// Recoverable preparation has finished before any outgoing state is cleared.
/// Menu and in-game loading share this commit; fresh startup admission is not run.
pub(crate) fn commit_prepared_load(
    state: &mut AppState,
    path: &std::path::Path,
    prepared: crate::app::persistence::PreparedSavedGame,
    from_menu: bool,
) {
    let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    crate::app::radar_movie::retire(state, wall_ms);
    crate::app::chrono_screen::retire(state, wall_ms);
    let native_tiberium_stats = prepared.native_tiberium_stats();
    log::info!(
        "Load: rebuilt native tiberium queues ({} growth, {} spread)",
        native_tiberium_stats.growth_entries,
        native_tiberium_stats.spread_entries,
    );

    // Ordinary native Load67F76C->751710->751765->406E80 clears outgoing
    // SoundEvents; UI69BB82->7535D0 resets EVA. TechnoLoad70C231..70C240
    // reconstructs each voice handle/latch. Do this only after preparation
    // succeeded; the new FootLoad state starts silent until its next AI visit.
    // Ordinary Vox.Save7533B0 emits tags only, no nodes or selected voice;
    // incoming announcements start empty. Saved static Voc is a separate path.
    if let Some(sfx) = state.audio.sfx_player.as_mut() {
        sfx.stop_all();
    }
    state.match_state.match_audio.reset_for_new_match();
    crate::app::reset_scenario_exit_runtime(state);
    crate::app::input::dispatch::selection_navigation::reset_for_world_replacement(
        &mut state.match_state.input,
    );
    // The outgoing replay closes before the process dummy and runtime change.
    crate::app::match_runtime::sim_tick::close_replay_segment_for_new_timeline(state);
    let committed = prepared.commit_into(
        &mut state.process_assets,
        &state.frontend.frontend_main_rng,
        &mut state.match_state.sim_runtime,
    );
    crate::app::audio_runtime::bind_match_eva_context(state);
    // All preparation has succeeded. Surface allocation and saved camera
    // conversion now use the tactical upscaler dimensions on either route.
    state.frontend.screen = crate::ui::game_screen::GameScreen::InGame;
    // A validated saved timeline replaces the outgoing fresh-L0 evidence and
    // remains available to the ordinary pump and exact diagnostic stepping.
    state.process_assets.refresh_campaigns_if_available();
    let campaign = state.process_assets.campaigns().and_then(|campaigns| {
        crate::app::match_bootstrap::PreparedCampaignStartup::from_saved_scenario(
            campaigns,
            &state
                .match_state
                .sim_runtime
                .as_ref()
                .expect("prepared saved game installs a runtime")
                .simulation,
            committed.map.source(),
        )
    });
    state.match_state.startup.acknowledge_saved_load(campaign);
    if from_menu {
        crate::app::loading::pump::present_game_mode_blank(state);
    }

    // Saved native identity is session.current_house. Never borrow an outgoing
    // sandbox preference or another match's human display handle.
    state.match_state.local_owner_override = None;
    // Load_Game 0x0067E4E8 restores GameMode from the save header; VERA's
    // persisted classification rides on the restored ScenarioSession.
    let game_mode = state
        .match_state
        .sim_runtime
        .as_ref()
        .expect("prepared saved game installs a runtime")
        .simulation
        .session()
        .game_mode();
    if game_mode == crate::sim::scenario_session::ScenarioGameMode::Campaign {
        // Successful ContentLoad67F7C1 suppresses a new initial save.
        state.persistence.loaded_campaign();
    }
    state
        .match_state
        .session
        .restore_from_save(game_mode, committed.map.local_player_name());
    state.match_state.scenario_elapsed_clock = committed.elapsed_clock;
    #[cfg(feature = "telemetry-dashboard")]
    crate::app::telemetry_capture::reset_observation(state);
    super::scenario_pause::load_clock(state, wall_ms);
    let pause_effects = state
        .match_state
        .sim_runtime
        .as_mut()
        .map_or_else(Vec::new, |rt| {
            crate::app::radar_movie::release_saved_scenario_hold(
                &mut rt.simulation,
                committed.map.radar_pause_credit(),
            )
        });
    // Load rearms614; reapply physical audio without a new pause acquisition.
    super::scenario_pause::resume(state, wall_ms);
    state.match_state.sandbox_full_visibility = false;
    let runtime = state
        .match_state
        .sim_runtime
        .as_ref()
        .expect("prepared saved game installs a runtime");
    let theater_name = runtime.simulation.session().theater().to_owned();
    let theater_ext = crate::app::loading::init_helpers::theater_ext_for(&theater_name).to_owned();
    let binary_frame = runtime.simulation.session().binary_frame();
    let map = crate::app::loading::transitions::MapPresentationInputs {
        campaign_names: Some(committed.map.campaign_names().clone()),
        basic: committed.map.basic().clone(),
        ranking: committed.map.ranking().clone(),
        source: committed.map.source().clone(),
        map_hash: Some(committed.map.map_hash()),
        terrain_grid: committed.terrain_grid,
        overlays: committed.details.occupied_overlays,
        terrain_objects: committed.map.terrain_objects().to_vec(),
        house_roster: committed.map.house_roster().clone(),
        lighting_config: committed.map.lighting().clone(),
        lighting_events: None,
        theater_name,
        theater_ext,
    };
    crate::app::loading::transitions::install_world_presentation(
        state,
        map,
        committed.presentation,
    );

    // MouseLoad5BE092 restores incoming Power; SidebarNoInit6A4F89/94
    // rearms only slide. Sample after world adoption so diagnostic time uses
    // the incoming tick too. Rejected preparation never reaches this commit.
    let power_ms = state.radar_presentation_ms();
    let mut power = committed.power_bar_anim;
    power.reanchor_after_load(power_ms);
    state.match_state.match_presentation.power_bar_anim = power;

    crate::app::audio_runtime::bind_match_theme_context(state);
    let music_now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    if let Some(assets) = state.process_assets.manager() {
        state.audio.restore_saved_theme(
            committed.map.theme_slots(),
            state.persistence.options_profile.in_game_music,
            assets,
            music_now_ms,
        );
    }

    // Successful-load685167..6851A1 reduces the retained action-line timer
    // against the restored binary frame, then reanchors that remainder.
    // The ordinary globals streams67F7E0/67F9C0 exclude this process timer.
    // Executed reanchor controls: tools/procedural_drawing_oracle/action_lines.
    state
        .match_state
        .match_presentation
        .target_lines
        .reanchor_after_load(binary_frame);
    state
        .match_state
        .match_presentation
        .sinking_waterlines
        .borrow_mut()
        .restore(committed.details.sinking_waterlines);
    state
        .match_state
        .match_presentation
        .lighting
        .restore_anim_intensities(committed.details.anim_intensities);
    // MouseLoad5BDF70->no-init5BE9B0 preserves the saved entry lists; their
    // display keys refresh below against incoming rules/CSF without reordering.
    #[cfg(feature = "phobos")]
    let sw_sidebar_expanded = committed
        .details
        .sidebar_order
        .as_ref()
        .map(|order| order.sw_sidebar_expanded());
    state
        .match_state
        .match_presentation
        .sidebar_projection
        .restore_order(committed.details.sidebar_order);
    #[cfg(feature = "phobos")]
    if let Some(expanded) = sw_sidebar_expanded {
        // `ScenarioExt::SWSidebar_Enable` survives the load like the strips.
        state.match_state.match_presentation.sw_sidebar.expanded = expanded;
    }

    let owner = state.match_state.local_player_owner().map(str::to_owned);
    state
        .match_state
        .match_presentation
        .sidebar_projection
        .restore_credits_counter(owner.as_deref(), committed.credits_counter);
    if let Some(owner) = owner {
        state
            .match_state
            .sim_runtime
            .as_mut()
            .expect("prepared saved game installs a runtime")
            .simulation
            .prepare_fog_view_for(&owner);
    }
    if let Some(shroud) = state.match_state.match_presentation.shroud_buffer.as_mut() {
        shroud.mark_stale();
    }
    if let Some(minimap) = state.match_state.match_presentation.minimap.as_mut() {
        minimap.mark_stale();
        minimap.restore_radar_events(committed.radar_events);
    }
    // Replaces every outgoing cursor mode, including the targeting and
    // Repair/Sell modes, with the saved Display's.
    committed.local_view.commit_into(
        &mut state.match_state.input,
        &mut state.match_state.match_presentation.sidebar_gadget_state,
    );
    crate::app::input::camera::restore_tactical_view(state, committed.details.tactical_view);

    // Reset pacing, not saved simulation input delay or elapsed score time.
    state.platform.frame_pacer.reset_for_immediate_frame();
    crate::app::presentation::sidebar_render::refresh_sidebar_projection(state);
    // The incoming chrome owns the footer allowance; refresh its layout
    // before initializing derived geometry, including on a cold/different-side
    // load. No Power AI visit or retained-band clamp belongs in this phase.
    let power_budget = crate::app::presentation::building_anim::power_bar_budget_px(state);
    state
        .match_state
        .match_presentation
        .power_bar_anim
        .set_max_segments(power_budget);
    log::debug!(
        "Load: restored Power at {power_ms}ms: {:?}",
        state.match_state.match_presentation.power_bar_anim,
    );
    // Apply after the incoming local view, so lock cleanup cannot be undone
    // by the saved targeting/cursor projection.
    super::sim_tick::apply_trigger_effects(state, &pause_effects);
    log::info!("Load: restored simulation from {}", path.display());
}
