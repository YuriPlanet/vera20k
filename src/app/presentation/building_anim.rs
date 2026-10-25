//! Building animation lifecycle, sidebar UI tick, and sound playback.
//!
//! The shared sound consumer runs at reached simulation boundaries and the
//! frame tail. Periodic animation/audio/UI updates follow simulation advancement.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::app::input::commands::preferred_local_owner_name;
use crate::sim::production;

/// The tactical listener shared by inline and frame-tail sound consumption.
pub(crate) fn sound_listener(state: &AppState) -> crate::audio::sfx::SpatialListener {
    let (tactical_width, tactical_height) =
        crate::app::input::camera::tactical_viewport_size(state);
    crate::audio::sfx::SpatialListener {
        tactical_width: tactical_width as i32,
        tactical_height: tactical_height as i32,
        origin_x: state.match_state.input.camera_x,
        origin_y: state.match_state.input.camera_y,
        zoom: state.match_state.input.zoom_level,
    }
}

/// Refresh retained750E20 sources using the same spatial decision owner
/// as inline delivery, before the common audio pump (including paused frames).
pub(crate) fn refresh_coordinate_sound_gains(state: &mut AppState) {
    let listener = sound_listener(state);
    let sim = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|runtime| &runtime.simulation);
    let local_owner = preferred_local_owner_name(state);
    let Some(catalog) = state.process_assets.audio_catalog() else {
        return;
    };
    let spatial =
        SoundSpatialContext::new(catalog.sounds(), &listener, sim, local_owner.as_deref());
    if let Some(sfx) = state.audio.sfx_player.as_mut() {
        sfx.refresh_coordinate_sound_gains(|key, source| spatial.gain_for(key, Some(source)));
    }
}

/// The cell test `VocClass::CalcVolumeAndPan` makes for a `Type=SHROUD`
/// sound (`0x00750B51..0x00750B9B`): the null cell `(0,0)` (the static
/// `[0x00B1D310]` pair) is always silent, and any other cell is silent while
/// both of the local player's ground bits `CellClass+0x12C & 0x18` are clear.
fn shroud_cell_silences(
    fog: &crate::sim::vision::FogState,
    owner: crate::sim::intern::InternedId,
    rx: u16,
    ry: u16,
) -> bool {
    (rx, ry) == (0, 0) || !fog.has_ground_bits(owner, rx, ry)
}

/// Borrowed process sources and listener for one synchronous sound delivery.
/// The simulation is supplied only for the duration of `consume`.
pub(crate) struct SoundPlaybackInputs<'a> {
    pub(crate) assets: &'a crate::assets::asset_manager::AssetManager,
    pub(crate) catalog: &'a crate::app::process_assets::ProcessAudioCatalog,
    pub(crate) listener: crate::audio::sfx::SpatialListener,
}

struct SoundSpatialContext<'a> {
    registry: &'a crate::rules::sound_ini::SoundRegistry,
    listener: &'a crate::audio::sfx::SpatialListener,
    sim: Option<&'a crate::sim::world::Simulation>,
    local_owner_id: Option<crate::sim::intern::InternedId>,
}

impl<'a> SoundSpatialContext<'a> {
    fn new(
        registry: &'a crate::rules::sound_ini::SoundRegistry,
        listener: &'a crate::audio::sfx::SpatialListener,
        sim: Option<&'a crate::sim::world::Simulation>,
        local_owner: Option<&str>,
    ) -> Self {
        Self {
            registry,
            listener,
            sim,
            local_owner_id: local_owner
                .and_then(|name| sim.and_then(|sim| sim.interner().get(name))),
        }
    }

    fn shrouded(&self, rx: u16, ry: u16) -> bool {
        match (self.sim, self.local_owner_id) {
            (Some(sim), Some(owner)) => shroud_cell_silences(sim.fog(), owner, rx, ry),
            _ => false,
        }
    }

    /// `None` is native volume below 0.05: no event playback. A retained
    /// Voc750E20 request still keeps its coordinate for the service phase.
    fn gain_for(
        &self,
        sound_id: &str,
        source: Option<crate::audio::events::SoundSource>,
    ) -> Option<crate::audio::sfx::SpatialGain> {
        use crate::audio::sfx::{SpatialGain, SpatialSource, spatial_gain};

        let Some(source) = source else {
            return Some(SpatialGain::CENTRED_FULL);
        };
        let facts = self.registry.get(sound_id).map_or_else(
            || SpatialSource::from_registry_defaults(self.registry),
            SpatialSource::from_entry,
        );
        let (rx, ry) = source.cell();
        spatial_gain(
            facts,
            source.screen_x,
            source.screen_y,
            self.listener,
            self.shrouded(rx, ry),
        )
    }
}

impl SoundPlaybackInputs<'_> {
    fn spatial_context<'s>(
        &'s self,
        sim: Option<&'s crate::sim::world::Simulation>,
        local_owner: Option<&str>,
    ) -> SoundSpatialContext<'s> {
        SoundSpatialContext::new(self.catalog.sounds(), &self.listener, sim, local_owner)
    }

    /// Foot4DADF0 reaches750D40 before Scatter and later callbacks. Use
    /// that coordinate and the existing handle, including a kept sound.
    pub(crate) fn update_looping_sound(
        &self,
        sfx: &mut crate::audio::sfx::SfxPlayer,
        sim: &crate::sim::world::Simulation,
        local_owner: Option<&str>,
        owner: u64,
        world: crate::sim::anim_class::AnimWorldCoord,
    ) {
        let Some(sound) = sfx.handle_sound_id(owner) else {
            return;
        };
        let source = crate::app::match_runtime::sound_dispatch::anim_world_sound_source(world);
        let gain = self
            .spatial_context(Some(sim), local_owner)
            .gain_for(&sound, Some(source));
        sfx.update_looping_sound(owner, gain, self.catalog.sounds());
    }

    /// Consume ordered requests through the sole SFX owner. Periodic voice,
    /// positional-owner and bomb maintenance stay in the frame-tail drain.
    pub(crate) fn consume(
        &self,
        sfx: &mut crate::audio::sfx::SfxPlayer,
        events: &[crate::audio::events::GameSoundEvent],
        sim: Option<&crate::sim::world::Simulation>,
        local_owner: Option<&str>,
    ) {
        use crate::audio::events::GameSoundEvent;
        use crate::audio::sfx::SpatialGain;

        let registry = self.catalog.sounds();
        let audio_indices = self.catalog.index();
        let eva_registry = self.catalog.eva();
        let assets = self.assets;
        let spatial = self.spatial_context(sim, local_owner);
        let gain_for = |sound_id: &str, source| spatial.gain_for(sound_id, source);
        // RulesClass::ReadAudioVisual @ 0x006691E0 (CloakSound) and
        // ObjectTypeClass::ReadINI @ 0x005F93B5 (CrushSound) store only the
        // VocClass::FindByName @ 0x007514D0 result, and PlayAtPos 0x00750920
        // indexes the same catalog. An invalid name is silent; it must not
        // enter the generic raw audio-bag fallback.
        let play_voc = |sfx: &mut crate::audio::sfx::SfxPlayer,
                        sound_id: &str,
                        source: Option<crate::audio::events::SoundSource>| {
            if registry.get(sound_id).is_some()
                && let Some(gain) = gain_for(sound_id, source)
            {
                sfx.play_registered_sound_spatial(sound_id, gain, registry);
            }
        };
        for event in events {
            match event {
                GameSoundEvent::UnitVoiceVisit { owner } => sfx.visit_unit_voice(*owner, registry),
                GameSoundEvent::UnitVoiceDestroyed { owner } => sfx.destroy_unit_voice(*owner),
                // Unit acknowledgement lines — always full volume (non-positional).
                // `TechnoClass::Queue_Voice @ 0x00708D90` only latches the line on
                // the speaking object. Only an actual reached Techno6F9EBB
                // effect above visits that latch, in the simulation's live order.
                GameSoundEvent::UnitSelected { speaker_id, .. }
                | GameSoundEvent::UnitMoveOrder { speaker_id, .. }
                | GameSoundEvent::UnitAttackOrder { speaker_id, .. }
                | GameSoundEvent::UnitVoiceRequested { speaker_id, .. } => {
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
                        assets,
                        audio_indices,
                    );
                }
                GameSoundEvent::TriggerEva { index } => {
                    let _ = sfx.queue_eva(
                        *index,
                        Some(crate::rules::sound_ini::EvaType::Interrupt),
                        None,
                        eva_registry,
                        assets,
                        audio_indices,
                    );
                }
                GameSoundEvent::VocIndex { index } => {
                    if let Some(entry) = registry.entry_by_index(*index) {
                        play_voc(sfx, &entry.id, None);
                    }
                }
                GameSoundEvent::EvaRemove { event: eva_event } => {
                    sfx.remove_eva(eva_event, eva_registry);
                }
                // UI events — always full volume (non-positional).
                GameSoundEvent::UiSound { .. } => {
                    sfx.play_sound(event.sound_id(), registry);
                }
                // `CreditsClass::Draw @ 0x004A2519`: `PUSH 0x3f000000` — the
                // credit tick is the one UI cue native plays at half volume,
                // centred (`EDX = 0x2000`).
                GameSoundEvent::CreditTick { .. } => {
                    sfx.play_sound_with_volume(event.sound_id(), 0.5, registry);
                }
                GameSoundEvent::AnimationStarted {
                    anim_id,
                    sound_id,
                    source,
                } => match gain_for(sound_id, *source) {
                    Some(gain) => {
                        sfx.play_animation_sound_spatial(*anim_id, sound_id, gain, registry);
                    }
                    None => sfx.bind_inaudible_animation_sound(*anim_id, sound_id, registry),
                },
                GameSoundEvent::AnimationReleased { anim_id } => {
                    sfx.release_animation_sound(*anim_id);
                }
                GameSoundEvent::AnimationDetached { anim_id } => {
                    sfx.detach_animation_sound(*anim_id);
                }
                GameSoundEvent::AnimationStopped {
                    anim_id,
                    stop_sound_id,
                    source,
                } => {
                    sfx.stop_animation_sound(*anim_id);
                    if let Some(stop_sound_id) =
                        stop_sound_id.as_deref().filter(|id| !id.is_empty())
                        && let Some(gain) = gain_for(stop_sound_id, *source)
                    {
                        sfx.play_sound_spatial(stop_sound_id, gain, registry);
                    }
                }
                GameSoundEvent::CloakSound { sound_id, source }
                | GameSoundEvent::WallCrushed { sound_id, source }
                | GameSoundEvent::VocAt { sound_id, source } => {
                    play_voc(sfx, sound_id, *source);
                }
                GameSoundEvent::VocAtCoord {
                    sound_id,
                    coordinate,
                    flags,
                    source,
                } => {
                    sfx.play_sound_at_coord(
                        sound_id,
                        *coordinate,
                        *flags,
                        *source,
                        gain_for(sound_id, Some(*source)),
                        registry,
                    );
                }
                GameSoundEvent::VocStopAtCoord { coordinate, flags } => {
                    sfx.stop_sound_at_coord(*coordinate, *flags);
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
                    );
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
                                    let _ =
                                        sfx.play_registered_sound_spatial(sound_id, gain, registry);
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
                                );
                            }
                        }
                    }
                    // `VoxClass::PlayEVA(name, -1)` at every `SuperClass::Launch`
                    // site: the entry's own `Type=`/`Priority=` route it.
                    if let Some(eva_event) = eva_event.as_deref().filter(|s| !s.is_empty()) {
                        let _ = sfx.play_eva(eva_event, None, eva_registry, assets, audio_indices);
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
                        sfx.play_sound_spatial(sound_id, gain, registry);
                    }
                    if let Some(eva_event) = eva_event.as_deref().filter(|s| !s.is_empty()) {
                        let _ = sfx.play_eva(eva_event, None, eva_registry, assets, audio_indices);
                    }
                }
                // Spatial events — distance volume and pan from the sound's
                // Range/Type/MinVolume against the tactical view.
                _ => {
                    if let Some(gain) = gain_for(event.sound_id(), event.source()) {
                        sfx.play_sound_spatial(event.sound_id(), gain, registry);
                    }
                }
            }
        }
    }
}

/// Tick the sidebar power bar animation (segment-by-segment transition).
pub(crate) fn update_power_bar_anim(state: &mut AppState, wall_ms: u64) {
    let owner_name = preferred_local_owner_name(state);
    let bar_height_px = power_bar_budget_px(state);

    let anim = &mut state.match_state.match_presentation.power_bar_anim;
    match (
        state.match_state.sim_runtime.as_ref(),
        owner_name.as_deref(),
    ) {
        (Some(runtime), Some(owner)) => {
            let sim = &runtime.simulation;
            let rules = &runtime.resources.rules;
            let theoretical_total = || sim.theoretical_power_for_owner(rules, owner);
            anim.fit_strip(bar_height_px, theoretical_total);
            let (produced, drained) = production::power_balance_for_owner(sim, rules, owner);
            anim.update(produced, drained, theoretical_total, wall_ms);
        }
        _ => {
            anim.fit_strip(bar_height_px, || 0);
            anim.update(0, 0, || 0, wall_ms);
        }
    }
}

/// Current Sidebar geometry, shared by ordinary AI and saved NoInit before
/// its first draw. The retained native band counts are never clamped to it.
pub(crate) fn power_bar_budget_px(state: &AppState) -> i32 {
    // Compute bar height from sidebar layout.
    let spec = state.match_state.match_presentation.sidebar_layout_spec;
    let (sw, sh) = state.sidebar_frame().layout_screen();
    let layout = crate::ui::sidebar::compute_layout_with_spec(spec, sw, sh, 0);
    // 63FB20: segment budget is (native strip height + 3) / 3.
    (layout.cameo_grid_bottom - layout.cameo_grid_top) as i32 + 3
}

/// House508DF0 -> Radar656BE0 -> Voc750920: centred, volume 1, NULL
/// handle. Both one-shots survive a reversal independently of the movie loop.
pub(crate) fn radar_availability_sound(
    rules: &crate::rules::ruleset::RuleSet,
    cue: crate::render::radar::anim::RadarAvailabilityCue,
) -> Option<crate::audio::events::GameSoundEvent> {
    use crate::render::radar::anim::RadarAvailabilityCue;
    let sound = match cue {
        RadarAvailabilityCue::On => &rules.general.radar_on,
        RadarAvailabilityCue::Off => &rules.general.radar_off,
    };
    sound
        .clone()
        .map(|sound_id| crate::audio::events::GameSoundEvent::VocAt {
            sound_id,
            source: None,
        })
}

/// Reconcile view/tool changes with the same radar owner the reached House
/// boundaries update. Its equality guard suppresses already-delivered edges.
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

    let cue = state
        .match_state
        .match_presentation
        .radar_anim
        .as_mut()
        .and_then(|radar| radar.set_has_radar(new_has_radar));
    if let Some(cue) = cue {
        //656BE0 ends the radar's movie loop before either cue.
        crate::app::radar_movie::end_radar_sound_loop(state);
        if let Some(sound) = state
            .rules()
            .and_then(|rules| radar_availability_sound(rules, cue))
        {
            state.match_state.match_audio.sound_events.push(sound);
        }
    }
}

/// Drain pending sound events from the queue and play them through the SFX player.
///
/// Voice events (VoiceSelect, VoiceMove, VoiceAttack, VoiceCapture, ...) are
/// latched per speaking object and visited at reached Techno AI heads — see
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
    drain_sound_events_inner(state, true);
}

/// Admit pending input effects before the frame's derived voice-interest
/// view. This does not redrive positional owners.
pub(crate) fn drain_pending_sound_events(state: &mut AppState) {
    drain_sound_events_inner(state, false);
}

fn drain_sound_events_inner(state: &mut AppState, redrive_owners: bool) {
    use crate::audio::sfx::{SpatialSource, spatial_gain};

    let events = state.match_state.match_audio.sound_events.drain();
    let listener = sound_listener(state);
    let local_owner = preferred_local_owner_name(state);
    let sim = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation);
    let ticking_sound = state
        .rules()
        .and_then(|rules| rules.general.bomb_ticking_sound.clone());
    let (Some(sfx), Some(assets), Some(catalog)) = (
        &mut state.audio.sfx_player,
        state.process_assets.manager(),
        state.process_assets.audio_catalog(),
    ) else {
        return;
    };
    let registry = catalog.sounds();

    // The EVA queue's dequeue is not driven here. `VoxClass::PlayNextQueued
    // @ 0x00752760` has three callers: the gated audio service (`0x00406FC1`),
    // admission (`0x0075257A`) and `PumpAndCheckActive` (`0x007529E0`), which
    // the blocking waits call. RESIDUAL: its radar caller (`0x00657A0C`, on the
    // draw where a queued sidebar clip starts or ends) is not ported; a line
    // queued on that frame starts up to one 33 ms service pass later.
    let inputs = SoundPlaybackInputs {
        assets,
        catalog,
        listener,
    };
    inputs.consume(sfx, &events, sim, local_owner.as_deref());
    let spatial = inputs.spatial_context(sim, local_owner.as_deref());
    let shrouded = |rx, ry| spatial.shrouded(rx, ry);

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
                sfx.play_animation_sound_spatial(owner, sound_id, gain, registry);
            }
        }
    }

    if !redrive_owners {
        return;
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
        // Techno6FA04F updates the turret handle at its reached AI visit.
        // A tail poll could restart it after a later Stop/Release boundary.
        if crate::sim::world::turret_rotate_sound_techno(owner).is_some() {
            continue;
        }
        // `ObjectClass::AI` replays an object's `+0x3C`/`+0x50` handles with
        // `VocClass::PlayAt` on each visit (the sim's `AnimationStarted`);
        // 750D40 never runs on them.
        if let Some(alive) = sim.object_sound_owner_alive(owner) {
            if !alive {
                sfx.stop_animation_sound(owner);
            }
            continue;
        }
        // Building43FC34 already updated +6B4 on its unchanged operational
        // branch, before later C4/destructor callbacks. Changed edges only
        // Detach/Play; neither branch admits a final-position update here.
        if crate::sim::world::working_sound_building(owner).is_some() {
            continue;
        }
        // Techno6F9EA8 and Foot4DADF0 re-drive their handles at the actual
        // AI visit. A tail poll could overwrite that position or restart a
        // loop after a later callback. Turret and Anim stay separate.
        if crate::sim::combat::gattling::gattling_sound_techno(owner).is_some()
            || sim.entities().get(owner).is_some_and(|entity| {
                use crate::map::entities::EntityCategory::{Aircraft, Infantry, Unit};
                matches!(entity.category(), Unit | Infantry | Aircraft)
            })
        {
            continue;
        }
        let Some(world) = sim.looping_sound_owner_coord(owner) else {
            // The owner object is gone. Native's `ObjectClass` uninit path
            // releases or stops its handle before the pointer can dangle.
            // Anim Destroy emits release, removing the owner handle while
            // leaving one-shot Reports playing; MoveSound emits stop. Those
            // handled owners never enter this fallback.
            sfx.stop_animation_sound(owner);
            continue;
        };
        let (rx, ry, sub_x, sub_y, _) = world.to_cell_sub_z();
        let (screen_x, screen_y) =
            crate::util::lepton::lepton_to_screen_exact_z(rx, ry, sub_x, sub_y, world.z);
        let facts = registry_facts_for_owner(registry, sfx, owner);
        let gain = facts
            .and_then(|facts| spatial_gain(facts, screen_x, screen_y, &listener, shrouded(rx, ry)));
        sfx.update_looping_sound(owner, gain, registry);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Either ground bit keeps a SHROUD cue audible, even where the shroud
    /// knowledge is closed; none, or the null cell, silences it.
    #[test]
    fn shroud_cues_follow_the_cell_ground_bits() {
        let owner = crate::sim::intern::test_intern("Americans");
        let mut fog = crate::sim::vision::FogState::new(32, 32);
        assert!(shroud_cell_silences(&fog, owner, 5, 5));
        fog.set_ground_bits_for_test(owner, 5, 5, 0x10);
        assert!(!shroud_cell_silences(&fog, owner, 5, 5));
        fog.set_ground_bits_for_test(owner, 0, 0, 0x18);
        assert!(shroud_cell_silences(&fog, owner, 0, 0));
    }
}
