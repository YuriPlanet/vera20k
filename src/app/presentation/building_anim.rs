//! Building animation lifecycle, sidebar UI tick, and sound playback.
//!
//! These are per-frame runtime updates that run after the sim tick advances.
//! Split from `match_runtime::sim_tick` to separate animation/audio/UI concerns from
//! core simulation advancement.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::app::input::commands::preferred_local_owner_name;
use crate::sim::production;

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
    let ticking_sound = state
        .rules()
        .and_then(|rules| rules.general.bomb_ticking_sound.clone());
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
            } => match gain_for(sound_id, *source) {
                Some(gain) => {
                    sfx.play_animation_sound_spatial(
                        *anim_id,
                        sound_id,
                        gain,
                        registry,
                        assets,
                        audio_indices,
                    );
                }
                None => sfx.bind_inaudible_animation_sound(*anim_id, sound_id, registry),
            },
            GameSoundEvent::AnimationReleased { anim_id } => {
                sfx.release_animation_sound(*anim_id);
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
            | GameSoundEvent::WallCrushed { sound_id, source }
            | GameSoundEvent::VocAt { sound_id, source } => {
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

    // `BombListClass::UpdateAll @ 0x00438BF0`'s ticking pass
    // (0x00438C6C..0x00438CFE): a carrier out of limbo starts
    // `BombTickingSound=` at its Location for every player (`VocClass::PlayAt`
    // with the bomb's own handle, 0x00438CCA) and re-drives it after that
    // (0x00438CF8); in limbo, spent or defused, the handle stops. Here the
    // start is taken for any carrier with no live handle; the owner loop below
    // re-drives and stops it through `bomb_ticking_coord`.
    // `RulesClass::ReadAudioVisual` keeps only `VocClass::FindByName`'s index:
    // an unregistered name is -1, which the ticking pass skips (0x00438C7F).
    let ticking_sound = ticking_sound.filter(|sound_id| registry.get(sound_id).is_some());
    if let (Some(sim), Some(sound_id)) = (sim, ticking_sound.as_deref()) {
        for &carrier in sim.bomb_carriers() {
            let owner = crate::sim::bomb::ticking_sound_owner(carrier);
            if sfx.loop_handle_sound_id(owner).is_some() {
                continue;
            }
            let Some(world) = sim.bomb_ticking_coord(carrier) else {
                continue;
            };
            let (rx, ry, sub_x, sub_y, _) = world.to_cell_sub_z();
            let (screen_x, screen_y) =
                crate::util::lepton::lepton_to_screen_exact_z(rx, ry, sub_x, sub_y, world.z);
            let facts = registry.get(sound_id).map_or_else(
                || SpatialSource::from_registry_defaults(registry),
                SpatialSource::from_entry,
            );
            if let Some(gain) = spatial_gain(facts, screen_x, screen_y, &listener, shrouded(rx, ry))
            {
                sfx.play_animation_sound_spatial(
                    owner,
                    sound_id,
                    gain,
                    registry,
                    assets,
                    audio_indices,
                );
            }
        }
    }

    // `AnimClass::UpdateLoopingSound @ 0x00750D40` from the owner's side.
    // Native's owner calls it on every one of its own updates with its
    // current coordinate: a positive volume re-drives `SoundEvent::SetVolume`
    // and `SetPan` and re-points the handle, and a volume that has fallen to
    // zero calls `SoundEvent::Stop` while the handle keeps its sound — which
    // silences a sustained cue whose object has driven out of earshot and
    // starts it again when the object comes back (or after a `Limit=` kill).
    // `SoundEvent::UpdateState @ 0x004057DC` reaps a stopped event on its next
    // pass because the owner no longer names it.
    for owner in sfx.looping_owners() {
        let Some(sim) = sim else {
            sfx.stop_animation_sound(owner);
            continue;
        };
        let Some(world) = sim.looping_sound_owner_coord(owner) else {
            // The owner object is gone. Native's `ObjectClass` uninit path
            // releases the handle before the pointer can dangle
            // (`release_move_sound` / `destroy_anim` push the stop event), so
            // this is the belt-and-braces arm.
            sfx.stop_animation_sound(owner);
            continue;
        };
        let (rx, ry, sub_x, sub_y, _) = world.to_cell_sub_z();
        let (screen_x, screen_y) =
            crate::util::lepton::lepton_to_screen_exact_z(rx, ry, sub_x, sub_y, world.z);
        let facts = registry_facts_for_owner(registry, sfx, owner);
        let gain = facts
            .and_then(|facts| spatial_gain(facts, screen_x, screen_y, &listener, shrouded(rx, ry)));
        sfx.update_looping_sound(owner, gain, registry, assets, audio_indices);
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
    let key = sfx.handle_sound_id(owner)?;
    Some(registry.get(&key).map_or_else(
        || crate::audio::sfx::SpatialSource::from_registry_defaults(registry),
        crate::audio::sfx::SpatialSource::from_entry,
    ))
}
