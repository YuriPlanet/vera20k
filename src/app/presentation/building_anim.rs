//! Building animation lifecycle, sidebar UI tick, and sound playback.
//!
//! These are per-frame runtime updates that run after the sim tick advances.
//! Split from `match_runtime::sim_tick` to separate animation/audio/UI concerns from
//! core simulation advancement.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use std::collections::HashMap;

use crate::app::AppState;
use crate::app::input::commands::preferred_local_owner_name;
use crate::app::types::SIM_TICK_MS;
use crate::sim::components::{AnimRuntime, GarrisonMuzzleFlash};
use crate::sim::production;
use crate::sim::world::Simulation;

const GARRISON_OCCUPANT_ANIM_Z_ADJUST: i32 = -200;

/// Advance the app-owned wall-clock terrain-overlay animation timer.
///
/// Building one-shot overlays now advance inside the authoritative simulation
/// frame; this independent timer only drives looping terrain presentation.
pub(crate) fn tick_terrain_overlay_animations(state: &mut AppState, dt_ms: u32) {
    state.match_state.match_presentation.idle_anim_elapsed_ms += dt_ms;
}

/// Tick the sidebar power bar animation (segment-by-segment transition).
pub(crate) fn update_power_bar_anim(state: &mut AppState) {
    let owner_name = preferred_local_owner_name(state);
    let (power_produced, power_drained) = match (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state.rules(),
        owner_name.as_deref(),
    ) {
        (Some(sim), Some(rules), Some(owner)) => {
            production::power_balance_for_owner(sim, rules, owner)
        }
        _ => (0, 0),
    };
    let theoretical = match (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        owner_name.as_deref(),
    ) {
        (Some(sim), Some(owner)) => production::theoretical_power_for_owner(sim, owner),
        _ => 0,
    };

    // Compute bar height from sidebar layout.
    let spec = state.match_state.match_presentation.sidebar_layout_spec;
    let sw = state.render_width() as f32;
    let sh = state.render_height() as f32;
    let layout = crate::sidebar::compute_layout_with_spec(spec, sw, sh, 0);
    // 63FB20: segment budget is (native strip height + 3) / 3.
    let bar_height_px = (layout.cameo_grid_bottom - layout.cameo_grid_top) as i32 + 3;

    state
        .match_state
        .match_presentation
        .power_bar_anim
        .set_max_segments(bar_height_px);
    state.match_state.match_presentation.power_bar_anim.update(
        power_produced,
        power_drained,
        theoretical,
    );
    state.match_state.match_presentation.power_bar_anim.tick();
}

/// Update radar availability from ECS and tick the radar chrome animation.
pub(crate) fn update_radar_state(state: &mut AppState) {
    let new_has_radar: bool = match (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state.rules(),
        preferred_local_owner_name(state).as_deref(),
    ) {
        (Some(sim), Some(rules), Some(owner)) => {
            crate::sim::radar::has_radar_for_owner(sim, rules, owner)
        }
        _ => false,
    };
    state.match_state.match_presentation.has_radar = new_has_radar;

    if let Some(ref mut ra) = state.match_state.match_presentation.radar_anim {
        ra.set_has_radar(new_has_radar);
    }
}

/// The EVA voice column for this session — `VoxClass::SetSide @ 0x007534E0`.
///
/// Native stores it once per scenario: `ScenarioClass::Full_Init`
/// (`0x00687801..0x00687807`) passes `Houses[PlayerIndex]->Type(+0x34)->Side
/// (+0xBC)` — the local country's `Side=` index — to `InitSideMixFiles @
/// 0x00534FA0`, which calls `SetSide` first (before its own `2 -> 1` MIX
/// remap, so a Yuri country keeps the Yuri column). A `PlayerIndex` of `-1`
/// or out of range passes 0. `EvaSide::from_side_index` is the column select
/// of `PlayNextQueued` (`0x007528E8..0x007528FE`).
///
/// VERA resolves the same value from the local house's `side_index`
/// (`HouseState`, from `rules.country_side_index`) at consume time; the
/// local owner is fixed for the match, so this equals a once-stored side.
pub(crate) fn local_eva_side(state: &AppState) -> crate::rules::sound_ini::EvaSide {
    use crate::rules::sound_ini::EvaSide;
    let side_index = preferred_local_owner_name(state).and_then(|owner| {
        let sim = &state.match_state.sim_runtime.as_ref()?.simulation;
        crate::sim::house_state::house_state_for_owner(&sim.houses, &owner, &sim.interner)
            .map(|house| i32::from(house.side_index))
    });
    EvaSide::from_side_index(side_index.unwrap_or(0))
}

/// Drain pending sound events from the queue and play them through the SFX player.
///
/// Voice events (VoiceSelect, VoiceMove, VoiceAttack, VoiceCapture, ...) are
/// latched per speaking object and drained at the end of the pass — see
/// [`crate::audio::voice_queue`] — so a repeated click drops the second line
/// instead of restarting the first mid-word. All other sounds go to the SFX
/// pool.
///
/// Positional cues go through `VocClass::CalcVolumeAndPan @ 0x00750AC0`
/// (`audio::sfx::spatial_gain`) against the tactical view rect — the native
/// listener is the tactical view (`0x00886FA8`/`0x00886FAC`), never the whole
/// window — with the local player's shroud standing in for the cell flags
/// `CellClass+0x12C & 0x18`.
///
/// Zoom is VERA-internal (gamemd has none). Sound positions are world pixels
/// and the viewport size is device pixels, so the listener carries the zoom
/// and `SpatialListener::view_extent` converts the rect into the world frame
/// the positions already use — see that method for why the world frame is the
/// side that is scaled.
pub(crate) fn drain_sound_events(state: &mut AppState) {
    use crate::audio::events::{GameSoundEvent, SoundSource};
    use crate::audio::sfx::{SpatialGain, SpatialListener, SpatialSource, spatial_gain};

    let events = state.match_state.match_audio.sound_events.drain();
    let (tactical_width, tactical_height) = crate::app::input::camera::tactical_viewport_size_px(
        state.render_width(),
        state.render_height(),
    );
    let listener = SpatialListener {
        tactical_width: tactical_width as i32,
        tactical_height: tactical_height as i32,
        origin_x: state.match_state.input.camera_x,
        origin_y: state.match_state.input.camera_y,
        zoom: state.match_state.input.zoom_level,
    };
    let local_owner = preferred_local_owner_name(state);
    let sim = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation);
    let local_owner_id = local_owner
        .as_deref()
        .and_then(|name| sim.and_then(|sim| sim.interner.get(name)));
    let eva_side = local_eva_side(state);
    let (Some(sfx), Some(assets)) = (&mut state.audio.sfx_player, state.process_assets.manager())
    else {
        return;
    };
    let registry = &state.audio.sound_registry;
    let audio_indices = &state.audio.audio_indices;
    let eva_registry = &state.audio.eva_registry;
    sfx.advance_voice_queue(registry, assets, audio_indices);

    // `CellClass+0x12C & 0x18 == 0`: neither explored nor visible. The shroud
    // renderer (`render::shroud_buffer`) blacks out the same cells — never
    // revealed, or re-shrouded by a hostile gap generator.
    let shrouded = |rx: u16, ry: u16| -> bool {
        match (sim, local_owner_id) {
            (Some(sim), Some(owner)) => {
                !sim.fog.is_cell_revealed(owner, rx, ry)
                    || sim.fog.is_cell_gap_covered(owner, rx, ry)
            }
            _ => false,
        }
    };
    // `None` is the native "volume below 0.05 -> nothing plays" outcome.
    let gain_for = |sound_id: &str, source: Option<SoundSource>| -> Option<SpatialGain> {
        let Some(source) = source else {
            return Some(SpatialGain::CENTRED_FULL);
        };
        let facts = registry.get(sound_id).map_or_else(
            || SpatialSource::from_registry_defaults(registry),
            SpatialSource::from_entry,
        );
        let (rx, ry) = source.cell();
        spatial_gain(
            facts,
            source.screen_x,
            source.screen_y,
            &listener,
            shrouded(rx, ry),
        )
    };

    for event in &events {
        match event {
            // Unit acknowledgement lines — always full volume (non-positional).
            // `TechnoClass::Queue_Voice @ 0x00708D90` only latches the line on
            // the speaking object; `drain_unit_voices` below is the
            // `TechnoClass::AI_Update @ 0x006F9EBB` drain that decides whether
            // it plays, is dropped as a repeat, or waits for the live line.
            GameSoundEvent::UnitSelected { speaker_id, .. }
            | GameSoundEvent::UnitMoveOrder { speaker_id, .. }
            | GameSoundEvent::UnitAttackOrder { speaker_id, .. } => {
                sfx.queue_unit_voice(*speaker_id, event.sound_id());
            }
            // `VoxClass::PlayEVA @ 0x00752700`: routing (pending slot, the
            // QUEUE FIFOs, critical/interrupt lists), the same-type duplicate
            // rule and the 500 ms gap all come from the entry and the
            // `VoxClass` state in `sfx` — see `audio::vox`.
            GameSoundEvent::Eva {
                event: eva_event,
                type_override,
            } => {
                let _ = sfx.play_eva(
                    eva_event,
                    *type_override,
                    eva_registry,
                    eva_side,
                    registry,
                    assets,
                    audio_indices,
                );
            }
            // UI events — always full volume (non-positional).
            GameSoundEvent::UiSound { .. } => {
                sfx.play_sound(event.sound_id(), registry, assets, audio_indices);
            }
            // `CreditsClass::Draw @ 0x004A2519`: `PUSH 0x3f000000` — the
            // credit tick is the one UI cue native plays at half volume,
            // centred (`EDX = 0x2000`).
            GameSoundEvent::CreditTick { .. } => {
                sfx.play_sound_with_volume(event.sound_id(), 0.5, registry, assets, audio_indices);
            }
            GameSoundEvent::AnimationStarted {
                anim_id,
                sound_id,
                source,
            } => {
                if let Some(gain) = gain_for(sound_id, *source) {
                    sfx.play_animation_sound_spatial(
                        *anim_id,
                        sound_id,
                        gain,
                        registry,
                        assets,
                        audio_indices,
                    );
                }
            }
            GameSoundEvent::AnimationStopped {
                anim_id,
                stop_sound_id,
                source,
            } => {
                sfx.stop_animation_sound(*anim_id);
                if let Some(stop_sound_id) = stop_sound_id.as_deref().filter(|id| !id.is_empty())
                    && let Some(gain) = gain_for(stop_sound_id, *source)
                {
                    sfx.play_sound_spatial(stop_sound_id, gain, registry, assets, audio_indices);
                }
            }
            GameSoundEvent::CloakSound { sound_id, source }
            | GameSoundEvent::WallCrushed { sound_id, source } => {
                // RulesClass::ReadAudioVisual @ 0x006691E0 (CloakSound) and
                // ObjectTypeClass::ReadINI @ 0x005F93B5 (CrushSound) store only
                // the VocClass::FindByName @ 0x007514D0 result. An invalid name
                // is silent; it must not enter the generic raw audio-bag fallback.
                if registry.get(sound_id).is_none() {
                    continue;
                }
                if let Some(gain) = gain_for(sound_id, *source) {
                    sfx.play_registered_sound_spatial(
                        sound_id,
                        gain,
                        registry,
                        assets,
                        audio_indices,
                    );
                }
            }
            GameSoundEvent::BaseUnderAttackSfx { sound_id } => {
                // `0x004F95BF MOV EDX,0x2000` / `0x004F95C4 PUSH 0x3F800000`:
                // pan centred, volume 1.0f, no handle — an ordinary pooled
                // event, not a voice. `RulesClass::ReadAudioVisual` keeps only
                // the `VocClass::FindByName` result, so an unregistered name
                // must not reach the raw audio-bag path.
                if registry.get(sound_id).is_none() {
                    continue;
                }
                sfx.play_registered_sound_spatial(
                    sound_id,
                    SpatialGain::CENTRED_FULL,
                    registry,
                    assets,
                    audio_indices,
                );
            }
            GameSoundEvent::BuildingDamagedSfx { sound_id, source } => {
                // `RulesClass::ReadAudioVisual @ 0x006691E0` keeps only the
                // `VocClass::FindByName` result in `Rules+0x714`, so a name
                // that is not a registered Voc is silence in gamemd and must
                // not reach the raw audio-bag fallback here.
                if registry.get(sound_id).is_none() {
                    continue;
                }
                if let Some(gain) = gain_for(sound_id, *source) {
                    sfx.play_registered_sound_spatial(
                        sound_id,
                        gain,
                        registry,
                        assets,
                        audio_indices,
                    );
                }
            }
            GameSoundEvent::VoiceFeedback { sound_id, source } => {
                // `CCINIClass::ReadSoundList @ 0x00525430` only stores tokens
                // `VocClass::FindPtrByName` resolves, so a `VoiceFeedback=`
                // name that is not a registered Voc never enters the vector at
                // `TechnoTypeClass+0x4D8` and must not reach the raw audio-bag
                // fallback here.
                //
                // Played as a positional one-shot, not through the
                // `VoiceQueue`: native's arm calls `VocClass::PlayAt @
                // 0x007509E0` (`0x00702709`) directly, with no
                // `TechnoClass::Queue_Voice @ 0x00708D90` latch and no
                // `+0x4DC` handle, so it neither cuts nor is cut by a
                // selection or order line.
                if registry.get(sound_id).is_none() {
                    continue;
                }
                if let Some(gain) = gain_for(sound_id, *source) {
                    sfx.play_registered_sound_spatial(
                        sound_id,
                        gain,
                        registry,
                        assets,
                        audio_indices,
                    );
                }
            }
            GameSoundEvent::LightningStrike { sound_id, source } => {
                // `CCINIClass::ReadSoundList @ 0x00525430` only stores tokens
                // `VocClass::FindPtrByName` resolves, so a `LightningSounds=`
                // name that is not a registered Voc never reaches the list and
                // must not fall through to the raw audio-bag path here.
                if registry.get(sound_id).is_none() {
                    continue;
                }
                if let Some(gain) = gain_for(sound_id, *source) {
                    sfx.play_registered_sound_spatial(
                        sound_id,
                        gain,
                        registry,
                        assets,
                        audio_indices,
                    );
                }
            }
            GameSoundEvent::SuperWeaponActivated {
                sound_id,
                source,
                eva_event,
            } => {
                // `RulesClass::ReadAudioVisual @ 0x006691E0` and
                // `SuperWeaponTypeClass::ReadINI @ 0x006CEBE5` both store only
                // the `VocClass::FindByName @ 0x007514D0` result, so a name
                // that is not a registered Voc is silence in gamemd and must
                // not reach the raw audio-bag fallback.
                if !sound_id.is_empty() && registry.get(sound_id).is_some() {
                    match source {
                        // `VocClass::PlayAtCoord @ 0x00750E20` /
                        // `PlayAt @ 0x007509E0` at the target coordinate.
                        Some(source) => {
                            if let Some(gain) = gain_for(sound_id, Some(*source)) {
                                let _ = sfx.play_registered_sound_spatial(
                                    sound_id,
                                    gain,
                                    registry,
                                    assets,
                                    audio_indices,
                                );
                            }
                        }
                        // `StormSound` only: `LightningStorm::Start` calls
                        // `VocClass::PlayAtPos @ 0x00750920` with pan `0x2000`
                        // and volume `1.0f` (`0x0053A03A`/`0x0053A03F`).
                        None => {
                            let _ = sfx.play_registered_sound_spatial(
                                sound_id,
                                SpatialGain::CENTRED_FULL,
                                registry,
                                assets,
                                audio_indices,
                            );
                        }
                    }
                }
                // `VoxClass::PlayEVA(name, -1)` at every `SuperClass::Launch`
                // site: the entry's own `Type=`/`Priority=` route it.
                if let Some(eva_event) = eva_event.as_deref().filter(|s| !s.is_empty()) {
                    let _ = sfx.play_eva(
                        eva_event,
                        None,
                        eva_registry,
                        eva_side,
                        registry,
                        assets,
                        audio_indices,
                    );
                }
            }
            GameSoundEvent::BridgeRepaired {
                sound_id,
                source,
                eva_event,
            } => {
                if !sound_id.is_empty()
                    && let Some(gain) = gain_for(sound_id, *source)
                {
                    sfx.play_sound_spatial(sound_id, gain, registry, assets, audio_indices);
                }
                if let Some(eva_event) = eva_event.as_deref().filter(|s| !s.is_empty()) {
                    let _ = sfx.play_eva(
                        eva_event,
                        None,
                        eva_registry,
                        eva_side,
                        registry,
                        assets,
                        audio_indices,
                    );
                }
            }
            // Spatial events — distance volume and pan from the sound's
            // Range/Type/MinVolume against the tactical view.
            _ => {
                if let Some(gain) = gain_for(event.sound_id(), event.source()) {
                    sfx.play_sound_spatial(event.sound_id(), gain, registry, assets, audio_indices);
                }
            }
        }
    }

    // `AnimClass::UpdateLoopingSound @ 0x00750D40` from the owner's side.
    // Native's owner calls it on every one of its own updates with its
    // current coordinate: a positive volume re-drives `SoundEvent::SetVolume`
    // and `SetPan` and re-points the handle, and a volume that has fallen to
    // zero calls `SoundEvent::Stop` and clears the handle — which is what
    // ends a sustained cue whose object has driven out of earshot.
    // `SoundEvent::UpdateState @ 0x004057DC` then reaps the event on its next
    // pass because the owner no longer names it.
    for owner in sfx.looping_owners() {
        let Some(sim) = sim else {
            sfx.update_looping_sound(owner, None);
            continue;
        };
        let Some(world) = sim.looping_sound_owner_coord(owner) else {
            // The owner object is gone. Native's `ObjectClass` uninit path
            // releases the handle before the pointer can dangle
            // (`release_move_sound` / `destroy_anim` push the stop event), so
            // this is the belt-and-braces arm.
            sfx.update_looping_sound(owner, None);
            continue;
        };
        let (rx, ry, sub_x, sub_y, z) = world.to_cell_sub_z();
        let (screen_x, screen_y) = crate::util::lepton::lepton_to_screen(rx, ry, sub_x, sub_y, z);
        let facts = registry_facts_for_owner(registry, sfx, owner);
        let gain = facts
            .and_then(|facts| spatial_gain(facts, screen_x, screen_y, &listener, shrouded(rx, ry)));
        sfx.update_looping_sound(owner, gain);
    }

    // `TechnoClass::AI_Update @ 0x006F9EBB` drains the per-object voice latch
    // immediately after its `AnimClass::UpdateLoopingSound` call at
    // `0x006F9EA8`, so the drain runs last here too. This is where a repeated
    // click is dropped instead of restarting the line mid-word.
    sfx.drain_unit_voices(registry, assets, audio_indices);
}

/// The `Range`/`Type`/`MinVolume` a live loop handle's entry carries, for the
/// per-update `CalcVolumeAndPan` re-drive.
fn registry_facts_for_owner(
    registry: &crate::rules::sound_ini::SoundRegistry,
    sfx: &crate::audio::sfx::SfxPlayer,
    owner: u64,
) -> Option<crate::audio::sfx::SpatialSource> {
    let key = sfx.loop_handle_sound_id(owner)?;
    Some(registry.get(&key).map_or_else(
        || crate::audio::sfx::SpatialSource::from_registry_defaults(registry),
        crate::audio::sfx::SpatialSource::from_entry,
    ))
}

/// Spawn new garrison muzzle flash animations from pending fire events and
/// advance existing ones. One-shot flashes are removed when their animation
/// completes.
///
/// Fire events with `garrison_muzzle_index` and `occupant_anim` produce a
/// short OccupantAnim SHP (e.g., UCFLASH) at the building's MuzzleFlash
/// pixel offset from art.ini.
pub(crate) fn tick_garrison_muzzle_flashes(state: &mut AppState, dt_ms: u32) {
    // Phase 1: spawn new flashes from pending fire events.
    let new_flashes: Vec<GarrisonMuzzleFlash> = {
        let sim = match state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)
        {
            Some(s) => s,
            None => {
                state
                    .match_state
                    .match_presentation
                    .garrison_muzzle_flashes
                    .clear();
                return;
            }
        };
        let art_reg = match state.rules().map(|rules| &rules.art_registry) {
            Some(a) => a,
            None => {
                state
                    .match_state
                    .match_presentation
                    .garrison_muzzle_flashes
                    .clear();
                return;
            }
        };
        let rules = match state.rules() {
            Some(r) => r,
            None => {
                state
                    .match_state
                    .match_presentation
                    .garrison_muzzle_flashes
                    .clear();
                return;
            }
        };
        let frame_counts = state
            .match_state
            .match_presentation
            .sprite_atlas
            .as_ref()
            .map(|atlas| &atlas.active_anim_frame_counts);
        state
            .match_state
            .match_presentation
            .pending_fire_effects
            .iter()
            .filter_map(|ev| {
                let anim_name = ev.occupant_anim.as_ref()?;
                let anim_section = sim.interner.resolve(*anim_name).to_ascii_uppercase();
                let origin = crate::app::presentation::fire_effects::resolve_fire_origin_from_sim(
                    sim, rules, art_reg, ev,
                )
                .ok()?;
                let runtime_config = art_reg.anim_runtime_config(&anim_section)?;
                let total_frames =
                    presentation_anim_frame_count(frame_counts, &anim_section).unwrap_or(1);
                Some(GarrisonMuzzleFlash {
                    building_id: ev.attacker_id,
                    runtime: garrison_occupant_anim_runtime(
                        &anim_section,
                        runtime_config,
                        total_frames,
                    ),
                    pixel_x: 0,
                    pixel_y: 0,
                    screen_x: origin.screen_x,
                    screen_y: origin.screen_y,
                    rx: origin.rx,
                    ry: origin.ry,
                    z: origin.z,
                    z_adjust: GARRISON_OCCUPANT_ANIM_Z_ADJUST,
                })
            })
            .collect()
    };
    state
        .match_state
        .match_presentation
        .garrison_muzzle_flashes
        .extend(new_flashes);

    // Phase 2: advance all flashes and remove finished ones. This is fed from
    // completed fixed sim ticks, not render-frame wall time.
    let Some((sim, art_reg)) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| (&rt.simulation, &rt.resources.rules.art_registry))
    else {
        state
            .match_state
            .match_presentation
            .garrison_muzzle_flashes
            .clear();
        return;
    };
    let empty_frame_counts = HashMap::new();
    let frame_counts = state
        .match_state
        .match_presentation
        .sprite_atlas
        .as_ref()
        .map(|atlas| &atlas.active_anim_frame_counts)
        .unwrap_or(&empty_frame_counts);
    state
        .match_state
        .match_presentation
        .garrison_muzzle_flashes
        .retain_mut(|flash| {
            advance_garrison_muzzle_flash(flash, dt_ms, sim, art_reg, frame_counts)
        });
}

fn advance_garrison_muzzle_flash(
    flash: &mut GarrisonMuzzleFlash,
    dt_ms: u32,
    sim: &Simulation,
    art_reg: &crate::rules::art_data::ArtRegistry,
    frame_counts: &HashMap<String, u16>,
) -> bool {
    flash.runtime.elapsed_logic_ms = flash.runtime.elapsed_logic_ms.saturating_add(dt_ms);
    while flash.runtime.elapsed_logic_ms >= SIM_TICK_MS && !flash.runtime.expired {
        flash.runtime.elapsed_logic_ms -= SIM_TICK_MS;
        advance_anim_runtime_visit(&mut flash.runtime, sim, art_reg, frame_counts);
    }
    !flash.runtime.expired
}

fn garrison_occupant_anim_runtime(
    anim_section: &str,
    config: &crate::rules::art_data::AnimTypeRuntimeConfig,
    total_frames: u16,
) -> AnimRuntime {
    let end = effective_anim_end(config, total_frames);
    let loop_end = effective_anim_loop_end(config, end);
    let reverse = config.reverse;
    AnimRuntime {
        type_name: anim_section.to_ascii_uppercase(),
        current_frame: if reverse { loop_end - 1 } else { 0 },
        frame_step: if reverse { -1 } else { 1 },
        delay_logic_frames: 0,
        reload_logic_frames: config.rate_logic_frames,
        rate_elapsed_logic_frames: 0,
        loop_remaining: native_loop_remaining(config.loop_count, 1),
        first_ai_guard: true,
        expired: false,
        constructor_reverse: false,
        elapsed_logic_ms: 0,
    }
}

#[cfg(test)]
fn garrison_occupant_anim_rate_logic_frames(
    sim: &Simulation,
    art_reg: &crate::rules::art_data::ArtRegistry,
    anim_name: crate::sim::intern::InternedId,
) -> Option<u16> {
    let anim_section = sim.interner.resolve(anim_name);
    art_reg
        .anim_runtime_config(anim_section)
        .map(|config| config.rate_logic_frames)
}

fn advance_anim_runtime_visit(
    runtime: &mut AnimRuntime,
    sim: &Simulation,
    art_reg: &crate::rules::art_data::ArtRegistry,
    frame_counts: &HashMap<String, u16>,
) {
    advance_anim_runtime_visit_with_events(runtime, sim, art_reg, frame_counts, None);
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AnimRuntimeVisitEvent {
    TrailerSpawn {
        parent_type: String,
        trailer_type: String,
    },
    NextInPlace {
        previous_type: String,
        next_type: String,
    },
    NormalDestroy {
        type_name: String,
    },
}

fn advance_anim_runtime_visit_with_events(
    runtime: &mut AnimRuntime,
    sim: &Simulation,
    art_reg: &crate::rules::art_data::ArtRegistry,
    frame_counts: &HashMap<String, u16>,
    mut events: Option<&mut Vec<AnimRuntimeVisitEvent>>,
) {
    if runtime.expired {
        return;
    }
    if let Some(config) = art_reg.anim_runtime_config(&runtime.type_name) {
        emit_anim_runtime_trailer(runtime, config, sim, &mut events);
    }
    if runtime.first_ai_guard {
        runtime.first_ai_guard = false;
        return;
    }
    if runtime.delay_logic_frames > 0 {
        runtime.delay_logic_frames -= 1;
        return;
    }
    if runtime.reload_logic_frames == 0 {
        return;
    }
    runtime.rate_elapsed_logic_frames = runtime.rate_elapsed_logic_frames.saturating_add(1);
    if runtime.rate_elapsed_logic_frames < runtime.reload_logic_frames {
        return;
    }
    runtime.rate_elapsed_logic_frames = 0;
    runtime.current_frame += runtime.frame_step;

    let Some(config) = art_reg.anim_runtime_config(&runtime.type_name) else {
        runtime.expired = true;
        return;
    };
    if config.ping_pong && anim_runtime_at_boundary(runtime, config, frame_counts) {
        runtime.frame_step = -runtime.frame_step;
        return;
    }
    if !anim_runtime_at_boundary(runtime, config, frame_counts) {
        return;
    }
    if runtime.loop_remaining != 0 && runtime.loop_remaining != u8::MAX {
        runtime.loop_remaining = runtime.loop_remaining.saturating_sub(1);
    }
    if runtime.loop_remaining != 0 {
        reset_anim_runtime_to_loop_start(runtime, config, frame_counts);
        return;
    }
    if let Some(next) = &config.next {
        switch_anim_runtime_type(runtime, next, art_reg, frame_counts, &mut events);
    } else {
        if let Some(events) = events.as_deref_mut() {
            events.push(AnimRuntimeVisitEvent::NormalDestroy {
                type_name: runtime.type_name.clone(),
            });
        }
        runtime.expired = true;
    }
}

fn emit_anim_runtime_trailer(
    runtime: &AnimRuntime,
    config: &crate::rules::art_data::AnimTypeRuntimeConfig,
    sim: &Simulation,
    events: &mut Option<&mut Vec<AnimRuntimeVisitEvent>>,
) {
    let Some(trailer_type) = &config.trailer_anim else {
        return;
    };
    if !anim_trailer_cadence_matches(sim.session.tick, config.trailer_seperation) {
        return;
    }
    if let Some(events) = events.as_deref_mut() {
        events.push(AnimRuntimeVisitEvent::TrailerSpawn {
            parent_type: runtime.type_name.clone(),
            trailer_type: trailer_type.clone(),
        });
    }
}

fn anim_trailer_cadence_matches(global_frame: u64, separation: i32) -> bool {
    separation == 1 || (global_frame as i32) % separation == 0
}

fn anim_runtime_at_boundary(
    runtime: &AnimRuntime,
    config: &crate::rules::art_data::AnimTypeRuntimeConfig,
    frame_counts: &HashMap<String, u16>,
) -> bool {
    let end = effective_anim_end(config, anim_total_frames(frame_counts, &runtime.type_name));
    let loop_end = effective_anim_loop_end(config, end);
    if runtime.frame_step >= 0 {
        let limit = if runtime.loop_remaining < 2 {
            end
        } else {
            loop_end - config.start
        };
        runtime.current_frame >= limit
    } else {
        let limit = if runtime.loop_remaining < 2 {
            config.start
        } else {
            config.loop_start - config.start
        };
        runtime.current_frame <= limit
    }
}

fn reset_anim_runtime_to_loop_start(
    runtime: &mut AnimRuntime,
    config: &crate::rules::art_data::AnimTypeRuntimeConfig,
    frame_counts: &HashMap<String, u16>,
) {
    if runtime.frame_step >= 0 && !runtime.constructor_reverse && !config.reverse {
        runtime.current_frame = config.loop_start - config.start;
    } else {
        let end = effective_anim_end(config, anim_total_frames(frame_counts, &runtime.type_name));
        runtime.current_frame = effective_anim_loop_end(config, end);
    }
}

fn switch_anim_runtime_type(
    runtime: &mut AnimRuntime,
    next: &str,
    art_reg: &crate::rules::art_data::ArtRegistry,
    frame_counts: &HashMap<String, u16>,
    events: &mut Option<&mut Vec<AnimRuntimeVisitEvent>>,
) {
    let Some(next_config) = art_reg.anim_runtime_config(next) else {
        runtime.expired = true;
        return;
    };
    let previous_type = runtime.type_name.clone();
    let total_frames = anim_total_frames(frame_counts, next);
    let end = effective_anim_end(next_config, total_frames);
    let loop_end = effective_anim_loop_end(next_config, end);
    let reverse = next_config.reverse || runtime.constructor_reverse;
    runtime.type_name = next.to_ascii_uppercase();
    runtime.current_frame = if reverse { loop_end - 1 } else { 0 };
    runtime.frame_step = if reverse { -1 } else { 1 };
    runtime.delay_logic_frames = 0;
    runtime.reload_logic_frames = next_config.rate_logic_frames;
    runtime.rate_elapsed_logic_frames = 0;
    runtime.loop_remaining = native_loop_remaining(next_config.loop_count, 1);
    runtime.first_ai_guard = false;
    runtime.expired = false;
    if let Some(events) = events.as_deref_mut() {
        events.push(AnimRuntimeVisitEvent::NextInPlace {
            previous_type,
            next_type: runtime.type_name.clone(),
        });
    }
}

fn native_loop_remaining(loop_count: i32, constructor_loop: u8) -> u8 {
    let raw = (loop_count as u8).wrapping_mul(constructor_loop.max(1));
    if raw < 2 { 1 } else { raw }
}

fn effective_anim_end(
    config: &crate::rules::art_data::AnimTypeRuntimeConfig,
    total_frames: u16,
) -> i32 {
    if config.end == -1 {
        let frames = i32::from(total_frames);
        if config.shadow { frames / 2 } else { frames }
    } else {
        config.end
    }
}

fn effective_anim_loop_end(
    config: &crate::rules::art_data::AnimTypeRuntimeConfig,
    effective_end: i32,
) -> i32 {
    if config.loop_end == -1 {
        effective_end
    } else {
        config.loop_end
    }
}

fn presentation_anim_frame_count(
    frame_counts: Option<&HashMap<String, u16>>,
    type_name: &str,
) -> Option<u16> {
    let frame_counts = frame_counts?;
    frame_counts.get(type_name).copied().or_else(|| {
        let canonical = type_name.to_ascii_uppercase();
        frame_counts.get(&canonical).copied()
    })
}

fn anim_total_frames(frame_counts: &HashMap<String, u16>, type_name: &str) -> u16 {
    presentation_anim_frame_count(Some(frame_counts), type_name).unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::art_data::{ArtRegistry, DEFAULT_ART_RATE_LOGIC_FRAMES};
    use crate::rules::ini_parser::IniFile;
    use crate::sim::world::Simulation;

    #[test]
    fn garrison_occupant_anim_rate_uses_art_section_rate_logic_frames() {
        let mut sim = Simulation::new();
        let ucflash = sim.interner.intern("UCFLASH");
        let art = ArtRegistry::from_ini(&IniFile::from_str("[UCFLASH]\nRate=300\n"));

        assert_eq!(
            garrison_occupant_anim_rate_logic_frames(&sim, &art, ucflash),
            Some(3)
        );
    }

    #[test]
    fn garrison_occupant_anim_rate_uses_animtype_default_logic_tick_when_rate_missing() {
        let mut sim = Simulation::new();
        let ucflash = sim.interner.intern("UCFLASH");
        let art = ArtRegistry::from_ini(&IniFile::from_str("[UCFLASH]\nFixtureOnly=1\n"));

        assert_eq!(
            garrison_occupant_anim_rate_logic_frames(&sim, &art, ucflash),
            Some(DEFAULT_ART_RATE_LOGIC_FRAMES)
        );
    }

    #[test]
    fn garrison_occupant_anim_rate_requires_art_section() {
        let mut sim = Simulation::new();
        let ucflash = sim.interner.intern("UCFLASH");
        let art = ArtRegistry::empty();

        assert_eq!(
            garrison_occupant_anim_rate_logic_frames(&sim, &art, ucflash),
            None
        );
    }

    #[test]
    fn garrison_muzzle_flash_first_ai_guard_does_not_advance_on_first_fixed_tick() {
        let sim = Simulation::new();
        let frame_counts = HashMap::from([("UCFLASH".to_string(), 3)]);
        let art = ArtRegistry::from_ini(&IniFile::from_str("[UCFLASH]\nEnd=-1\n"));
        let config = art.anim_runtime_config("UCFLASH").unwrap();
        let mut flash = GarrisonMuzzleFlash {
            building_id: 1,
            runtime: garrison_occupant_anim_runtime("UCFLASH", config, 3),
            pixel_x: 0,
            pixel_y: 0,
            screen_x: 0.0,
            screen_y: 0.0,
            rx: 0,
            ry: 0,
            z: 0,
            z_adjust: GARRISON_OCCUPANT_ANIM_Z_ADJUST,
        };

        assert!(advance_garrison_muzzle_flash(
            &mut flash,
            SIM_TICK_MS,
            &sim,
            &art,
            &frame_counts,
        ));
        assert_eq!(flash.runtime.current_frame, 0);
        assert!(!flash.runtime.first_ai_guard);
        assert_eq!(flash.runtime.elapsed_logic_ms, 0);
    }

    #[test]
    fn garrison_muzzle_flash_omitted_end_does_not_play_to_shp_frame_count() {
        let sim = Simulation::new();
        let frame_counts = HashMap::from([("UCFLASH".to_string(), 3)]);
        let art = ArtRegistry::from_ini(&IniFile::from_str("[UCFLASH]\nFixtureOnly=1\n"));
        let config = art.anim_runtime_config("UCFLASH").unwrap();
        let mut flash = GarrisonMuzzleFlash {
            building_id: 1,
            runtime: garrison_occupant_anim_runtime("UCFLASH", config, 3),
            pixel_x: 0,
            pixel_y: 0,
            screen_x: 0.0,
            screen_y: 0.0,
            rx: 0,
            ry: 0,
            z: 0,
            z_adjust: GARRISON_OCCUPANT_ANIM_Z_ADJUST,
        };

        assert!(advance_garrison_muzzle_flash(
            &mut flash,
            SIM_TICK_MS,
            &sim,
            &art,
            &frame_counts,
        ));
        assert!(!advance_garrison_muzzle_flash(
            &mut flash,
            SIM_TICK_MS,
            &sim,
            &art,
            &frame_counts,
        ));
        assert!(flash.runtime.expired);
        assert_eq!(flash.runtime.current_frame, 1);
    }

    #[test]
    fn garrison_muzzle_flash_rate_zero_never_advances() {
        let sim = Simulation::new();
        let frame_counts = HashMap::from([("UCFLASH".to_string(), 3)]);
        let art = ArtRegistry::from_ini(&IniFile::from_str("[UCFLASH]\nEnd=-1\nRate=0\n"));
        let config = art.anim_runtime_config("UCFLASH").unwrap();
        let mut flash = GarrisonMuzzleFlash {
            building_id: 1,
            runtime: garrison_occupant_anim_runtime("UCFLASH", config, 3),
            pixel_x: 0,
            pixel_y: 0,
            screen_x: 0.0,
            screen_y: 0.0,
            rx: 0,
            ry: 0,
            z: 0,
            z_adjust: GARRISON_OCCUPANT_ANIM_Z_ADJUST,
        };

        assert!(advance_garrison_muzzle_flash(
            &mut flash,
            SIM_TICK_MS * 4,
            &sim,
            &art,
            &frame_counts,
        ));
        assert_eq!(flash.runtime.current_frame, 0);
        assert!(!flash.runtime.expired);
    }

    #[test]
    fn garrison_muzzle_flash_loopcount_ff_is_infinite_sentinel() {
        let sim = Simulation::new();
        let frame_counts = HashMap::from([("UCFLASH".to_string(), 3)]);
        let art = ArtRegistry::from_ini(&IniFile::from_str(
            "[UCFLASH]\nEnd=2\nLoopStart=0\nLoopEnd=2\nLoopCount=-1\n",
        ));
        let config = art.anim_runtime_config("UCFLASH").unwrap();
        let mut flash = GarrisonMuzzleFlash {
            building_id: 1,
            runtime: garrison_occupant_anim_runtime("UCFLASH", config, 3),
            pixel_x: 0,
            pixel_y: 0,
            screen_x: 0.0,
            screen_y: 0.0,
            rx: 0,
            ry: 0,
            z: 0,
            z_adjust: GARRISON_OCCUPANT_ANIM_Z_ADJUST,
        };

        assert!(advance_garrison_muzzle_flash(
            &mut flash,
            SIM_TICK_MS * 3,
            &sim,
            &art,
            &frame_counts,
        ));
        assert_eq!(flash.runtime.loop_remaining, u8::MAX);
        assert_eq!(flash.runtime.current_frame, 0);
        assert!(!flash.runtime.expired);
    }

    #[test]
    fn garrison_muzzle_flash_next_switches_same_runtime() {
        let sim = Simulation::new();
        let frame_counts = HashMap::from([("UCFLASH".to_string(), 2), ("MYNEXT".to_string(), 2)]);
        let art = ArtRegistry::from_ini(&IniFile::from_str(
            "[UCFLASH]\nEnd=1\nNext=MYNEXT\n[MYNEXT]\nEnd=-1\n",
        ));
        let config = art.anim_runtime_config("UCFLASH").unwrap();
        let mut flash = GarrisonMuzzleFlash {
            building_id: 1,
            runtime: garrison_occupant_anim_runtime("UCFLASH", config, 2),
            pixel_x: 0,
            pixel_y: 0,
            screen_x: 0.0,
            screen_y: 0.0,
            rx: 0,
            ry: 0,
            z: 0,
            z_adjust: GARRISON_OCCUPANT_ANIM_Z_ADJUST,
        };

        assert!(advance_garrison_muzzle_flash(
            &mut flash,
            SIM_TICK_MS * 2,
            &sim,
            &art,
            &frame_counts,
        ));
        assert_eq!(flash.runtime.type_name, "MYNEXT");
        assert_eq!(flash.runtime.current_frame, 0);
        assert!(!flash.runtime.expired);
    }

    #[test]
    fn anim_runtime_trailer_emits_before_first_ai_guard_and_frame_advance() {
        let mut sim = Simulation::new();
        sim.session.tick = 6;
        let frame_counts = HashMap::from([("PARENT".to_string(), 3)]);
        let art = ArtRegistry::from_ini(&IniFile::from_str(
            "[PARENT]\nEnd=2\nRate=100\nTrailerAnim=SMOKEY2\nTrailerSeperation=2\n",
        ));
        let config = art.anim_runtime_config("PARENT").unwrap();
        let mut runtime = garrison_occupant_anim_runtime("PARENT", config, 3);
        let mut events = Vec::new();

        advance_anim_runtime_visit_with_events(
            &mut runtime,
            &sim,
            &art,
            &frame_counts,
            Some(&mut events),
        );

        assert_eq!(
            events,
            vec![AnimRuntimeVisitEvent::TrailerSpawn {
                parent_type: "PARENT".to_string(),
                trailer_type: "SMOKEY2".to_string(),
            }]
        );
        assert_eq!(runtime.current_frame, 0);
        assert!(!runtime.first_ai_guard);
        assert!(!runtime.expired);
    }

    #[test]
    fn anim_runtime_trailer_cadence_uses_signed_global_frame_modulo() {
        assert!(anim_trailer_cadence_matches(7, 1));
        assert!(anim_trailer_cadence_matches(10, -5));
        assert!(!anim_trailer_cadence_matches(11, -5));
    }

    #[test]
    fn anim_runtime_trailer_uses_old_type_before_next_and_not_new_type_same_visit() {
        let mut sim = Simulation::new();
        sim.session.tick = 8;
        let frame_counts = HashMap::from([("OLDANIM".to_string(), 2), ("NEXTANIM".to_string(), 2)]);
        let art = ArtRegistry::from_ini(&IniFile::from_str(
            "[OLDANIM]\nEnd=1\nRate=900\nNext=NEXTANIM\nTrailerAnim=OLDTRAIL\nTrailerSeperation=1\n\
             [NEXTANIM]\nEnd=1\nRate=900\nTrailerAnim=NEWTRAIL\nTrailerSeperation=1\n",
        ));
        let config = art.anim_runtime_config("OLDANIM").unwrap();
        let mut runtime = garrison_occupant_anim_runtime("OLDANIM", config, 2);
        runtime.first_ai_guard = false;
        let mut events = Vec::new();

        advance_anim_runtime_visit_with_events(
            &mut runtime,
            &sim,
            &art,
            &frame_counts,
            Some(&mut events),
        );

        assert_eq!(
            events,
            vec![
                AnimRuntimeVisitEvent::TrailerSpawn {
                    parent_type: "OLDANIM".to_string(),
                    trailer_type: "OLDTRAIL".to_string(),
                },
                AnimRuntimeVisitEvent::NextInPlace {
                    previous_type: "OLDANIM".to_string(),
                    next_type: "NEXTANIM".to_string(),
                },
            ]
        );
        assert_eq!(runtime.type_name, "NEXTANIM");
        assert_eq!(runtime.current_frame, 0);
        assert!(!runtime.first_ai_guard);
        assert!(!runtime.expired);
    }

    #[test]
    fn anim_runtime_normal_destroy_does_not_emit_bounce_or_expire_anim_outputs() {
        let mut sim = Simulation::new();
        sim.session.tick = 9;
        let frame_counts = HashMap::from([("BOOM".to_string(), 2)]);
        let art = ArtRegistry::from_ini(&IniFile::from_str(
            "[BOOM]\nEnd=1\nRate=900\nBounceAnim=BOUNCEFX\nExpireAnim=EXPIREFX\n",
        ));
        let config = art.anim_runtime_config("BOOM").unwrap();
        let mut runtime = garrison_occupant_anim_runtime("BOOM", config, 2);
        runtime.first_ai_guard = false;
        let mut events = Vec::new();

        advance_anim_runtime_visit_with_events(
            &mut runtime,
            &sim,
            &art,
            &frame_counts,
            Some(&mut events),
        );

        assert_eq!(
            events,
            vec![AnimRuntimeVisitEvent::NormalDestroy {
                type_name: "BOOM".to_string(),
            }]
        );
        assert!(runtime.expired);
    }
}
