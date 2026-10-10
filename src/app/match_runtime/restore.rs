//! Successful same-content world replacement and its presentation lifecycle.

use crate::app::AppState;

/// Apply the enumerated post-prepare replacement bundle. This function has no
/// recoverable failure path; best-effort presentation rebuilds retain their
/// prior valid resources when replacement is unavailable.
pub(crate) fn commit_prepared_load(
    state: &mut AppState,
    path: &std::path::Path,
    prepared: crate::app::persistence::PreparedLoad,
) {
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
    // Saved static Voc/EVA queues have separate, currently unported restoration.
    if let Some(sfx) = state.audio.sfx_player.as_mut() {
        sfx.stop_all();
    }
    state.match_state.match_audio.reset_for_new_match();
    crate::app::reset_scenario_exit_runtime(state);
    crate::app::input::dispatch::selection_navigation::reset_for_world_replacement(
        &mut state.match_state.input,
    );
    // F10 lifecycle: a successful in-scenario load closes the outgoing
    // timeline's diagnostic segment BEFORE the restored simulation commits;
    // the restored timeline lazily opens its own segment on the next frame.
    // A failed close discards rather than retains (no mixed-header artifact).
    crate::app::match_runtime::sim_tick::close_replay_segment_for_new_timeline(state);
    let runtime = state
        .match_state
        .sim_runtime
        .as_mut()
        .expect("prepared load requires a live runtime");
    // The close above reads the outgoing tick and its pre-recorded replay only;
    // it never observes the shared CellClass dummy reconstructed by this commit.
    let committed = prepared.commit_into(runtime);
    // Successful-load685167..6851A1 reduces the retained action-line timer
    // against the restored binary frame, then reanchors that remainder.
    // This does not introduce timer serialization or a new 25-frame flash.
    // The ordinary globals streams67F7E0/67F9C0 exclude this process timer.
    // Their restored frame precedes the685120 tail. Original stream layouts
    // and executed reanchor controls: tools/procedural_drawing_oracle/action_lines.
    state
        .match_state
        .match_presentation
        .target_lines
        .reanchor_after_load(runtime.simulation.session.binary_frame);
    state
        .match_state
        .match_presentation
        .sinking_waterlines
        .borrow_mut()
        .restore(committed.sinking_waterlines);
    state
        .match_state
        .match_presentation
        .barrel_image_pitches
        .borrow_mut()
        .clear();
    crate::app::loading::transitions::sync_in_game_options_speed_from_sim(state);
    state.match_state.match_presentation.combat_lights.clear();
    state.match_state.match_presentation.lasers.clear_on_load();
    if let Some(runtime) = state.match_state.sim_runtime.as_ref() {
        state
            .match_state
            .match_presentation
            .detail
            .borrow_mut()
            .configure_normal(&runtime.resources.rules.general.detail);
    }
    state
        .match_state
        .match_presentation
        .line_trails
        .clear_on_load();
    state
        .match_state
        .match_presentation
        .hidden_super_anims
        .clear();
    state
        .match_state
        .match_presentation
        .super_timer_blinks
        .clear();
    // MouseLoad5BDF70->no-init5BE9B0 preserves the saved entry lists rather
    // than re-sorting the current types. Their keys are refreshed from live
    // rules/CSF below without changing surviving order. A headless snapshot
    // has no saved strip and keeps the existing silent first-seed behavior.
    state
        .match_state
        .match_presentation
        .sidebar_projection
        .restore_order(committed.sidebar_order);
    crate::app::match_runtime::sim_tick::upsert_occupied_overlay_render_entries(
        state,
        committed.occupied_overlays,
    );

    // F10: prepare the restored session.current_house's view BEFORE the first
    // tactical render (the owner query derives from the committed simulation),
    // and invalidate the render dirty-gates: the view generation restarts from
    // zero, so an equal counter no longer proves an unchanged view.
    if let Some(owner) = crate::app::input::commands::preferred_local_owner_name(state) {
        if let Some(sim) = state
            .match_state
            .sim_runtime
            .as_mut()
            .map(|rt| &mut rt.simulation)
        {
            sim.prepare_fog_view_for(&owner);
        }
    }
    if let Some(shroud) = state.match_state.match_presentation.shroud_buffer.as_mut() {
        shroud.mark_stale();
    }
    if let Some(minimap) = state.match_state.match_presentation.minimap.as_mut() {
        minimap.mark_stale();
    }
    state
        .match_state
        .match_presentation
        .installed_playfield_authority = None;

    // Rebuild sprite/unit atlases so all entity types in the loaded save have
    // atlas entries before the first render frame.
    crate::app::match_runtime::sim_tick::refresh_entity_atlases(state);

    // The presentation owner cancels the old timeline's pending light samples.
    if let Some(runtime) = state.match_state.sim_runtime.as_ref()
        && let Some(terrain) = runtime.resources.terrain_template.as_ref()
    {
        let presentation = &mut state.match_state.match_presentation;
        presentation.lighting.restore(
            terrain,
            &runtime.simulation,
            &runtime.resources.rules,
            presentation.in_game_options.detail_level,
        );
    }

    // Reset timing to prevent a burst of ticks after the load.
    state.platform.frame_pacer.reset_for_immediate_frame();

    // Close the save/load panel after loading.

    // Same-content restoration leaves match-owned startup admission untouched.
    state.persistence.last_loaded_save_path = Some(path.to_path_buf());
    crate::app::presentation::sidebar_render::refresh_sidebar_projection(state);
    log::info!("Load: restored simulation from {}", path.display());
}
