//! Interpret ordered simulation sound events for the local listener.
//!
//! Owns app-side cue selection, spatial projection and ordered event expansion.
//! Simulation admission predicates remain in sim; device playback remains in audio.
//! This boundary can be exercised without constructing an AppState or audio device.

use super::eva_producers;
use crate::audio::events::{GameSoundEvent, SoundEventQueue, SoundSource};
use crate::rules::ruleset::RuleSet;
use crate::sim::world::{SimSoundEvent, Simulation};

#[cfg(test)]
#[path = "bridge_child_sound_tests.rs"]
mod bridge_child_sound_tests;

#[cfg(test)]
#[path = "selection_voice_tests.rs"]
mod selection_voice_tests;

/// Adapt the Simulation-owned normal Select voice request to the existing
/// audio event queue. Its Main RNG admission and draw belong to Simulation;
/// device playback and pending voice replacement remain in audio.
pub(crate) fn selection_voice_event(
    sim: &mut Simulation,
    rules: &RuleSet,
    entity_id: u64,
    voices_enabled: bool,
) -> Option<GameSoundEvent> {
    let sound_id = sim.selection_voice_request(rules, entity_id, voices_enabled)?;
    Some(GameSoundEvent::UnitSelected {
        speaker_id: entity_id,
        sound_id: sound_id.to_string(),
    })
}

/// QueueMegaMission6FFD42's default command acknowledgement. Simulation
/// owns the ordered-list Main draw and QueueVoice708D90 admission; the
/// existing unit voice queue owns pending replacement and eventual playback.
pub(crate) fn default_order_voice_event(
    sim: &mut Simulation,
    rules: &RuleSet,
    entity_id: u64,
    voices_enabled: bool,
) -> Option<GameSoundEvent> {
    let sound_id = sim.default_order_voice_request(rules, entity_id, voices_enabled)?;
    Some(GameSoundEvent::UnitMoveOrder {
        speaker_id: entity_id,
        sound_id: sound_id.to_string(),
    })
}

/// Append each already-admitted event in producer order, including multi-cue
/// events. Simulation owns feedback admission and its Main draws at damage
/// time; this adapter never advances a random stream.
///
/// `admit_radar` is the local client's `CreateRadarEvent @ 0x0065FA70`. Caller
/// admission can already belong to the producer: `StructureAbandoned` carries
/// the simulation's `House50B6F0` admission. Legacy arms still compare the local
/// owner's name before calling `admit_radar`. Each reached request calls it at
/// most once, in producer order; its result gates the caller's EVA line. That
/// ordering belongs to the caller, not to radar events.
///
/// RESIDUAL: the remaining legacy owner tests compare the local owner's name.
/// Native `0x0050B6F0` accepts either the human or `PlayerControl` flag in
/// campaign (`GameMode == 0`). Trigger: a campaign human/player-controlled
/// house whose name differs from the local owner; effect: those legacy arms
/// drop its radar events and EVA lines. Campaign play is not supported yet.
pub(super) fn dispatch_sim_sound_events(
    events: impl IntoIterator<Item = SimSoundEvent>,
    sim: &Simulation,
    rules: &RuleSet,
    local_owner_name: Option<&str>,
    admit_radar: &mut dyn FnMut(crate::sim::radar::RadarEventRequest) -> bool,
    output: &mut SoundEventQueue,
) {
    // Convert sim sound events to app-layer sound events for playback.
    for sim_event in events {
        let app_event: GameSoundEvent = match sim_event {
            SimSoundEvent::UnitVoiceVisit { owner } => GameSoundEvent::UnitVoiceVisit { owner },
            SimSoundEvent::UnitVoiceDestroyed { owner } => {
                GameSoundEvent::UnitVoiceDestroyed { owner }
            }
            SimSoundEvent::ObjectSoundStarted {
                owner,
                sound_id,
                world,
            } => GameSoundEvent::AnimationStarted {
                anim_id: owner,
                sound_id: sim.interner.resolve(sound_id).to_string(),
                source: Some(anim_world_sound_source(world)),
            },
            SimSoundEvent::AnimationStarted {
                anim_id,
                sound_id,
                world,
            } => GameSoundEvent::AnimationStarted {
                anim_id,
                sound_id,
                source: Some(anim_world_sound_source(world)),
            },
            SimSoundEvent::AnimationStopped {
                anim_id,
                stop_sound_id,
                world,
            } => GameSoundEvent::AnimationStopped {
                anim_id,
                stop_sound_id: stop_sound_id.map(|id| sim.interner.resolve(id).to_string()),
                source: Some(anim_world_sound_source(world)),
            },
            // The techno's `+0x4A4` handle: an owner loop, keyed apart from
            // every anim and object id (`combat::gattling`).
            SimSoundEvent::GattlingLoop {
                owner,
                sound_id,
                world,
            } => GameSoundEvent::AnimationStarted {
                anim_id: owner,
                sound_id,
                source: Some(anim_world_sound_source(world)),
            },
            SimSoundEvent::GattlingLoopStop { owner } => GameSoundEvent::AnimationStopped {
                anim_id: owner,
                stop_sound_id: None,
                source: None,
            },
            SimSoundEvent::GattlingLoopRelease { owner }
            | SimSoundEvent::ObjectSoundReleased { owner } => {
                GameSoundEvent::AnimationReleased { anim_id: owner }
            }
            SimSoundEvent::ObjectSoundDetached { owner } => {
                GameSoundEvent::AnimationDetached { anim_id: owner }
            }
            SimSoundEvent::AircraftPhase { sound_id, world } => GameSoundEvent::AircraftPhase {
                sound_id: sim.interner.resolve(sound_id).to_string(),
                source: Some(anim_world_sound_source(world)),
            },
            SimSoundEvent::EntityDied {
                die_sound_id,
                rx,
                ry,
            } => GameSoundEvent::EntityDestroyed {
                sound_id: die_sound_id,
                source: Some(sound_source_at_cell(rx, ry)),
            },
            SimSoundEvent::EntityCrushed {
                crush_sound_id,
                rx,
                ry,
            } => GameSoundEvent::EntityCrushed {
                sound_id: sim.interner.resolve(crush_sound_id).to_string(),
                source: Some(sound_source_at_cell(rx, ry)),
            },
            SimSoundEvent::EntityDeployed {
                deploy_sound_id,
                rx,
                ry,
            } => GameSoundEvent::EntityDeployed {
                sound_id: sim.interner.resolve(deploy_sound_id).to_string(),
                source: Some(sound_source_at_cell(rx, ry)),
            },
            SimSoundEvent::LeaveTransport { sound_id, rx, ry } => GameSoundEvent::LeaveTransport {
                sound_id: sim.interner.resolve(sound_id).to_string(),
                source: Some(sound_source_at_cell(rx, ry)),
            },
            SimSoundEvent::EntityUndeployed {
                undeploy_sound_id,
                rx,
                ry,
            } => GameSoundEvent::EntityUndeployed {
                sound_id: sim.interner.resolve(undeploy_sound_id).to_string(),
                source: Some(sound_source_at_cell(rx, ry)),
            },
            SimSoundEvent::ChronoTeleport { sound_id, rx, ry } => GameSoundEvent::ChronoTeleport {
                sound_id: sim.interner.resolve(sound_id).to_string(),
                source: Some(sound_source_at_cell(rx, ry)),
            },
            SimSoundEvent::UnitPromoted {
                owner,
                sound_id,
                elite: _,
                rx,
                ry,
            } => {
                // `HouseClass::IsHumanPlayer @ 0x0050B6F0`: only the
                // local player's own promotion is audible.
                if !owner_is_local(&sim.interner, owner, local_owner_name) {
                    continue;
                }
                // Native order: the positional `VocClass::PlayAt` first
                // (`0x006FA0BC` elite / `0x006FA12A` veteran), then
                // `VoxClass::Play("EVA_UnitPromoted")` (`0x006FA0CB` /
                // `0x006FA139`). Both are queued here in that order,
                // so the arm pushes its own events and never falls
                // through to the shared push below.
                if let Some(sound_id) = sound_id {
                    output.push(GameSoundEvent::UnitPromoted {
                        sound_id: sim.interner.resolve(sound_id).to_string(),
                        source: Some(sound_source_at_cell(rx, ry)),
                    });
                }
                // `0x006FA0C6 MOV ECX,0x843138 ("EVA_UnitPromoted") ; OR
                // EDX,-1 ; CALL VoxClass::PlayEVA`: the voice slot, with
                // the entry's own type (STANDARD LOW on stock data).
                output.push(GameSoundEvent::Eva {
                    event: "EVA_UnitPromoted".to_string(),
                    type_override: None,
                });
                continue;
            }
            SimSoundEvent::CloakSound {
                sound_id,
                rx,
                ry,
                sub_x,
                sub_y,
                world_z_leptons,
            } => cloak_sound_for_app(sound_id, rx, ry, sub_x, sub_y, world_z_leptons),
            SimSoundEvent::WallCrushed {
                sound_id,
                rx,
                ry,
                sub_x,
                sub_y,
                world_z_leptons,
            } => {
                let (sx, sy) = crate::util::lepton::lepton_to_screen_exact_z(
                    rx,
                    ry,
                    sub_x,
                    sub_y,
                    world_z_leptons,
                );
                GameSoundEvent::WallCrushed {
                    sound_id,
                    source: Some(SoundSource::new((sx, sy), (rx, ry))),
                }
            }
            SimSoundEvent::VocCentered { sound_id } => GameSoundEvent::VocAt {
                sound_id,
                // The existing Voc path resolves registered sounds only;
                // its absent source selects centre pan and full volume.
                source: None,
            },
            SimSoundEvent::VocAt {
                sound_id,
                audible_to,
                rx,
                ry,
                sub_x,
                sub_y,
                world_z_leptons,
            } => {
                // `HouseClass::IsHumanPlayer @ 0x0050B6F0`: the capture sound
                // plays only when the local player owns the firer or the
                // target.
                if let Some(houses) = audible_to
                    && !houses
                        .iter()
                        .any(|&house| owner_is_local(&sim.interner, house, local_owner_name))
                {
                    continue;
                }
                let (sx, sy) = crate::util::lepton::lepton_to_screen_exact_z(
                    rx,
                    ry,
                    sub_x,
                    sub_y,
                    world_z_leptons,
                );
                GameSoundEvent::VocAt {
                    sound_id,
                    source: Some(SoundSource::new((sx, sy), (rx, ry))),
                }
            }
            SimSoundEvent::BuildingComplete { owner } => {
                // Only play EVA for the local player's production.
                if !owner_is_local(&sim.interner, owner, local_owner_name) {
                    continue;
                }
                // `StripClass::AI 0x006A8E2F`: `PlayEVA` with type -1.
                GameSoundEvent::Eva {
                    event: "EVA_ConstructionComplete".to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::SuperWeaponLaunched {
                owner,
                sw_type,
                rx,
                ry,
            } => {
                // `SuperClass::Launch @ 0x006CC390` switches on the
                // launched type's `Type=` index and plays that case's
                // cue and/or EVA line. `superweapon_launch_cue` is that
                // table; both halves come off the one type object, as
                // native's `param_1[10]` does.
                let type_name = sim.interner.resolve(sw_type);
                let Some(sw) = rules.super_weapon(type_name) else {
                    continue;
                };
                let cue =
                    superweapon_launch_cue(sw.kind, &rules.general, sw.start_sound.as_deref());
                // The EVA column is picked for the *listening* client's
                // side, not the launcher's: `VoxClass::PlayEVA` takes
                // only the event name; the consumer resolves the side.
                let eva_event = cue
                    .eva_event
                    .filter(|_| local_owner_name.is_some())
                    .map(str::to_string);
                let activated = (cue.sound_id.is_some() || eva_event.is_some()).then(|| {
                    GameSoundEvent::SuperWeaponActivated {
                        sound_id: cue.sound_id.unwrap_or_default(),
                        source: cue.positional.then(|| sound_source_at_cell(rx, ry)),
                        eva_event,
                    }
                });
                // The player's tail ([`launch_drops_ready_line`]): after
                // the case's line, the queued Ready line is dropped.
                if let Some(ready) = launch_drops_ready_line(sw.kind)
                    && owner_is_local(&sim.interner, owner, local_owner_name)
                {
                    if let Some(activated) = activated {
                        output.push(activated);
                    }
                    output.push(GameSoundEvent::EvaRemove {
                        event: ready.to_string(),
                    });
                    continue;
                }
                let Some(activated) = activated else {
                    continue;
                };
                activated
            }
            SimSoundEvent::SuperWeaponRadarEvent { radar } => {
                // A type-13 event from `SuperClass::Launch` case 1
                // (`0x006CCF2F`), case 4 (`0x006CC4BE`, `0x006CC4D2`: the
                // source cell, then the target's), case 7 (`0x006CCDD7`) and
                // case 9 (`0x006CD8E0`), a storm's start (`0x00539F89`) or a
                // `NUKE` warhead's impact (`0x00467EA7`), with no local-player
                // test; it plays nothing.
                let _ = admit_radar(radar);
                continue;
            }
            SimSoundEvent::LightningStormBegan => {
                // The undeferred half of `LightningStorm::Start`, which a
                // launch's countdown defers: `[AudioVisual] StormSound=`
                // (`[Rules+0x730]`) through `VocClass::PlayAtPos @
                // 0x00750920` (`0x0053A044`) with pan `0x2000` and volume
                // 1.0, centred rather than at the storm cell. Its line
                // follows through `super_weapon_messages`; the EVA line
                // was spoken at launch, the whole countdown earlier.
                let Some(sound_id) = rules.general.storm_sound.clone() else {
                    continue;
                };
                GameSoundEvent::SuperWeaponActivated {
                    sound_id,
                    source: None,
                    eva_event: None,
                }
            }
            SimSoundEvent::LightningStormApproaching => {
                // `LightningStorm::Process @ 0x0053AB11`: `VoxClass::PlayEVA
                // @ 0x00752700` of `EVA_LightningStormCreated` on every
                // client; as for the launch's EVA, an app with no local
                // player plays none (the line follows through
                // `super_weapon_messages`).
                if local_owner_name.is_none() {
                    continue;
                }
                GameSoundEvent::Eva {
                    event: "EVA_LightningStormCreated".to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::UnitComplete { owner, radar } => {
                if !owner_is_local(&sim.interner, owner, local_owner_name) {
                    continue;
                }
                // `HouseClass::Place_Production 0x004FB631`: the type-6 radar
                // accept gates the line; `0x004FB644` plays it with type -1.
                if !admit_radar(radar) {
                    continue;
                }
                GameSoundEvent::Eva {
                    event: "EVA_UnitReady".to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::MatchOutcome { owner, kind } => {
                if !owner_is_local(&sim.interner, owner, local_owner_name) {
                    continue;
                }
                GameSoundEvent::Eva {
                    event: outcome_eva_event(kind).to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::WallSold { receiver } => {
                let receiver_name = sim.interner.resolve(receiver);
                let Some(event) =
                    wall_sell_sound_for_local(receiver_name, local_owner_name, Some(rules))
                else {
                    continue;
                };
                event
            }
            SimSoundEvent::CannotDeployHere { owner } => {
                if !owner_is_local(&sim.interner, owner, local_owner_name) {
                    continue;
                }
                // `UnitClass::Deploy 0x0073950A`: type -1.
                GameSoundEvent::Eva {
                    event: "EVA_CannotDeployHere".to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::ProductionRefused { owner } => {
                // `FactoryClass::StartProduction 0x004C9D3F..0x004C9D5F`:
                // ScoldSound through `0x00750920` for the player's house.
                if !owner_is_local(&sim.interner, owner, local_owner_name) {
                    continue;
                }
                let Some(sound_id) = rules.general.scold_sound.clone() else {
                    continue;
                };
                GameSoundEvent::UiSound { sound_id }
            }
            SimSoundEvent::BuildingPlaced { owner } => {
                // `0x004FB2CC..0x004FB314`: BuildingSlam through `0x00750920`
                // (pan `0x2000`, volume `1.0f`) for the player's house.
                if !owner_is_local(&sim.interner, owner, local_owner_name) {
                    continue;
                }
                let Some(sound_id) = rules.general.building_slam.clone() else {
                    continue;
                };
                GameSoundEvent::UiSound { sound_id }
            }
            SimSoundEvent::StructureGarrisoned { owner } => {
                // EVA cue: only play for the local human player.
                if !owner_is_local(&sim.interner, owner, local_owner_name) {
                    continue;
                }
                // `BuildingClass::AddGarrisonOccupant 0x005229C1`: type -1
                // (stock entry QUEUE NORMAL).
                GameSoundEvent::Eva {
                    event: "EVA_StructureGarrisoned".to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::StructureAbandoned { radar, .. } => {
                //458200's producer already ran House50B6F0. Native gates
                //EVA on successful radar admission, before refreshing art.
                if !admit_radar(radar) {
                    continue;
                }
                GameSoundEvent::Eva {
                    event: "EVA_StructureAbandoned".to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::BuildingGarrisonedSfx { owner, rx, ry } => {
                // Positional SFX: only audible to the local human player
                // (matches gamemd VocClass::PlayAt with IsHumanPlayer gate).
                if !owner_is_local(&sim.interner, owner, local_owner_name) {
                    continue;
                }
                let sound_id = match Some(rules)
                    .and_then(|r| r.general.building_garrisoned_sound.as_deref())
                {
                    Some(s) if !s.is_empty() => s.to_string(),
                    _ => continue,
                };
                GameSoundEvent::BuildingGarrisonedSfx {
                    sound_id,
                    source: Some(sound_source_at_cell(rx, ry)),
                }
            }
            SimSoundEvent::ChuteSound { rx, ry } => {
                let sound_id = match Some(rules).and_then(|r| r.general.chute_sound.as_deref()) {
                    Some(s) if !s.is_empty() => s.to_string(),
                    _ => continue,
                };
                GameSoundEvent::ChuteSound {
                    sound_id,
                    source: Some(sound_source_at_cell(rx, ry)),
                }
            }
            SimSoundEvent::C4Planted { rx, ry } => GameSoundEvent::C4Planted {
                sound_id: "SealPlaceBomb".to_string(),
                source: Some(sound_source_at_cell(rx, ry)),
            },
            SimSoundEvent::BuildingDamagedSfx { rx, ry } => {
                // `[AudioVisual] BuildingDamageSound` (`Rules+0x714`),
                // played at the building's own coordinate by
                // `BuildingClass::ReceiveDamage @ 0x00442700` /
                // `0x00442706 CALL VocClass::PlayAtCoord @ 0x00750E20`.
                // `RulesClass::ReadAudioVisual` stores only the
                // `VocClass::FindByName` result, so an absent or empty
                // key is silence rather than a bag lookup.
                let sound_id = match rules.general.building_damage_sound.as_deref() {
                    Some(s) if !s.is_empty() => s.to_string(),
                    _ => continue,
                };
                GameSoundEvent::BuildingDamagedSfx {
                    sound_id,
                    source: Some(sound_source_at_cell(rx, ry)),
                }
            }
            SimSoundEvent::BunkerWallsUp { rx, ry } => {
                // Walls-up cue on install; skip when the rules key is empty.
                let sound_id =
                    match Some(rules).and_then(|r| r.general.bunker_walls_up_sound.as_deref()) {
                        Some(s) if !s.is_empty() => s.to_string(),
                        _ => continue,
                    };
                GameSoundEvent::BunkerWalls {
                    sound_id,
                    source: Some(sound_source_at_cell(rx, ry)),
                }
            }
            SimSoundEvent::BunkerWallsDown { rx, ry } => {
                // Walls-down cue on normal exit / clear teardown.
                let sound_id =
                    match Some(rules).and_then(|r| r.general.bunker_walls_down_sound.as_deref()) {
                        Some(s) if !s.is_empty() => s.to_string(),
                        _ => continue,
                    };
                GameSoundEvent::BunkerWalls {
                    sound_id,
                    source: Some(sound_source_at_cell(rx, ry)),
                }
            }
            SimSoundEvent::BridgeRepaired {
                rx,
                ry,
                owner: _,
                radar,
            } => {
                // Spatial SFX gated on rules.bridge_rules.repair_sound
                // being set (the original game gates on
                // `RulesClass+0x248 != -1`).
                let sound_id = Some(rules)
                    .and_then(|r| r.bridge_rules.repair_sound.clone())
                    .unwrap_or_default();
                let source = if sound_id.is_empty() {
                    None
                } else {
                    Some(sound_source_at_cell(rx, ry))
                };
                // Infantry519BC9 calls EVA only after House50B6F0 (the sim
                // published `radar`) and radar insertion `0x00519BB6`
                // admitted it. A second owner filter here would incorrectly
                // suppress mode-0 PlayerControl output.
                let eva_event = radar
                    .is_some_and(|request| admit_radar(request))
                    .then(|| "EVA_BridgeRepaired".to_string());
                if sound_id.is_empty() && eva_event.is_none() {
                    continue;
                }
                GameSoundEvent::BridgeRepaired {
                    sound_id,
                    source,
                    eva_event,
                }
            }
            SimSoundEvent::UnderAttack {
                owner,
                miner,
                radar,
                ..
            } => {
                // Diamond and voice are both for the LOCAL player only.
                let is_local = owner_is_local(&sim.interner, owner, local_owner_name);
                if !is_local || !admit_radar(radar) {
                    continue;
                }
                // `HouseClass::NotifyUnderAttack 0x004F94FB/0x004F95B3`,
                // `UnitClass::ReceiveDamage 0x00738530`: `PlayEVA` type
                // -1 (stock entries STANDARD NORMAL → pending slot).
                // The radar accept (`CreateRadarEvent`) is native's only
                // rate limit.
                let cue = if miner {
                    "EVA_OreMinerUnderAttack"
                } else {
                    "EVA_OurBaseIsUnderAttack"
                };
                output.push(GameSoundEvent::Eva {
                    event: cue.to_string(),
                    type_override: None,
                });
                if let Some(siren) = base_under_attack_siren(miner, rules) {
                    output.push(siren);
                }
                continue;
            }
            SimSoundEvent::AllyUnderAttack { owner, radar } => {
                // `NotifyUnderAttack 0x004F95A0..0x004F95CF`: the type-16
                // radar accept, then the ally line for the LOCAL listener
                // and the same siren tail as the base line.
                if !owner_is_local(&sim.interner, owner, local_owner_name) || !admit_radar(radar) {
                    continue;
                }
                output.push(GameSoundEvent::Eva {
                    event: "EVA_OurAllyIsUnderAttack".to_string(),
                    type_override: None,
                });
                if let Some(siren) = base_under_attack_siren(false, rules) {
                    output.push(siren);
                }
                continue;
            }
            SimSoundEvent::UnitLost { owner, radar } => {
                // `TechnoClass::Death_Announcement 0x004D98FE..0x004D9911`:
                // `PlayEVA("EVA_UnitLost", -1)` for the local owner once
                // the sim's Spawned gate and this radar type-7 accept passed.
                if !owner_is_local(&sim.interner, owner, local_owner_name) || !admit_radar(radar) {
                    continue;
                }
                GameSoundEvent::Eva {
                    event: "EVA_UnitLost".to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::HouseEva { owner, event } => {
                // `HouseClass::Update 0x004F8BA0` / `0x004F8D14`:
                // `PlayEVA(name, -1)` — the entry's own `Type=` and
                // `Priority=` route it (stock: `EVA_InsufficientFunds`
                // STANDARD NORMAL, `EVA_LowPower` QUEUE IMPORTANT).
                if !owner_is_local(&sim.interner, owner, local_owner_name) {
                    continue;
                }
                GameSoundEvent::Eva {
                    event: event.to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::StructureSold { owner } => {
                // `BuildingClass::Mission_Selling 0x00449CC1 MOV AL,[EBP+0x41A]`
                // (`0x0044AB22` on the upgrade path): only the local
                // player's own building speaks; `EDX = -1`.
                if !owner_is_local(&sim.interner, owner, local_owner_name) {
                    continue;
                }
                GameSoundEvent::Eva {
                    event: eva_producers::EVA_STRUCTURE_SOLD.to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::SellClick { owner } => {
                // `BuildingClass::Sell_Back 0x0044719B CALL 0x0050B6F0`
                // (IsHumanPlayer), then `VocClass::PlayAtPos 0x00750920` of
                // `Rules+0x70C` (`0x004471B6`).
                if !owner_is_local(&sim.interner, owner, local_owner_name) {
                    continue;
                }
                let Some(sound_id) = rules.general.generic_click_sound.clone() else {
                    continue;
                };
                GameSoundEvent::UiSound { sound_id }
            }
            SimSoundEvent::Repairing { owner } => {
                // `BuildingClass::ToggleRepair 0x004470A4 CALL
                // 0x0050B6F0` / depot MissionRepair44C4F3 local+41A.
                if !owner_is_local(&sim.interner, owner, local_owner_name) {
                    continue;
                }
                GameSoundEvent::Eva {
                    event: eva_producers::EVA_REPAIRING.to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::UnitRepaired { owner, radar } => {
                // MissionRepair44BD96's local+41A test precedes its type8
                // radar insertion44BDB2; admission gates the voice44BDC5.
                if !owner_is_local(&sim.interner, owner, local_owner_name) || !admit_radar(radar) {
                    continue;
                }
                GameSoundEvent::Eva {
                    event: "EVA_UnitRepaired".to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::BuildingCaptured {
                old_owner,
                new_owner,
                tech_building,
                radar,
                capture_eva_event,
            } => {
                // `BuildingClass::ChangeOwner 0x004483C6/0x004483D1
                // CALL 0x0050B6F0` on the old and the new owner; the
                // lines are `eva_producers::capture_eva_events`. Every
                // call passes `EDX = -1` (the `0x00448459 QueueVoice`
                // passes `-1, -1` too).
                let local = local_owner_name;
                let local_is_old = owner_is_local(&sim.interner, old_owner, local);
                let local_is_new = owner_is_local(&sim.interner, new_owner, local);
                // `0x00448477 CreateRadarEvent(10)` is reached only past the
                // old-or-new-owner `0x0050B6F0` tests (`0x004483C6`,
                // `0x004483D1`; flag read at `0x004483EF`).
                let radar_accepted = (local_is_old || local_is_new)
                    && radar.is_some_and(|request| admit_radar(request));
                let capture_event = capture_eva_event.map(|id| sim.interner.resolve(id));
                for event in eva_producers::capture_eva_events(
                    tech_building,
                    radar_accepted,
                    capture_event,
                    local_is_old,
                    local_is_new,
                ) {
                    output.push(GameSoundEvent::Eva {
                        event,
                        type_override: None,
                    });
                }
                continue;
            }
            SimSoundEvent::SuperWeaponReady { owner, sw_type } => {
                // `HouseClass::Update 0x004F8E42 CMP ESI,[PlayerPtr] ;
                // SETZ CL ; PUSH ECX` is `SuperClass::AI_Ready`'s
                // announce argument (`0x006CBDCF TEST CL,CL`).
                if !owner_is_local(&sim.interner, owner, local_owner_name) {
                    continue;
                }
                let type_name = sim.interner.resolve(sw_type);
                let Some(event) = rules
                    .super_weapon(type_name)
                    .and_then(|sw| eva_producers::super_weapon_ready_event(sw.kind))
                else {
                    continue;
                };
                GameSoundEvent::Eva {
                    event: event.to_string(),
                    type_override: None,
                }
            }
            // The selection clear is `super_selection`'s; the pass plays
            // nothing.
            SimSoundEvent::SuperWeaponStatusChanged { .. } => continue,
            // The refusal's line follows through `super_weapon_messages`;
            // its AddMessage plays the only sound.
            SimSoundEvent::LightningStormRefused { .. }
            | SimSoundEvent::PsychicDominatorRefused { .. } => continue,
            SimSoundEvent::SuperWeaponDetected { owner, sw_type } => {
                // `BuildingClass::OnConstructionComplete
                // 0x004468AD..0x00446995`; the gates are the
                // listener's (`eva_producers::super_weapon_detected_allowed`)
                // and the line comes off the `[SuperWeaponTypes]`
                // list index (`0x00446948` jump table).
                let Some(local_name) = local_owner_name else {
                    continue;
                };
                let owner_name = sim.interner.resolve(owner).to_string();
                let type_name = sim.interner.resolve(sw_type).to_string();
                let local_defeated = sim
                    .interner
                    .get(local_name)
                    .and_then(|id| sim.houses.get(&id))
                    .is_some_and(|house| house.is_defeated);
                // `0x004468FA..0x00446935`: `AuxBuilding=` absent, or
                // the building's own house owns one
                // (`CountOwnedInstances` on `Owner+0x5550`).
                let aux_satisfied = rules.super_weapon(&type_name).is_none_or(|sw| {
                    crate::sim::superweapon::aux_building_present(sim, rules, owner, sw)
                });
                if !eva_producers::super_weapon_detected_allowed(
                    &sim.house_alliances,
                    &owner_name,
                    local_name,
                    local_defeated,
                    sim.session.game_mode_nonzero,
                    aux_satisfied,
                ) {
                    continue;
                }
                let Some(event) = rules
                    .super_weapon_order
                    .iter()
                    .position(|section| section.eq_ignore_ascii_case(&type_name))
                    .and_then(eva_producers::super_weapon_detected_event)
                else {
                    continue;
                };
                GameSoundEvent::Eva {
                    event: event.to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::PlayerDefeated { house } => {
                // `HouseClass::MPlayer_Defeated 0x004FC1B5 CMP EAX,ESI`
                // splits the local branch (`EVA_YouHaveLost`, owned by
                // `MatchOutcome`) from the `0x004FC3BC` line.
                if owner_is_local(&sim.interner, house, local_owner_name) {
                    continue;
                }
                GameSoundEvent::Eva {
                    event: eva_producers::EVA_PLAYER_DEFEATED.to_string(),
                    type_override: None,
                }
            }
        };
        output.push(app_event);
    }
}

fn wall_sell_sound_for_local(
    receiver_name: &str,
    local_owner: Option<&str>,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> Option<GameSoundEvent> {
    if !local_owner.is_some_and(|local| local.eq_ignore_ascii_case(receiver_name)) {
        return None;
    }
    Some(GameSoundEvent::UiSound {
        sound_id: rules?.general.sell_sound.clone()?,
    })
}

/// The `[AudioVisual] BaseUnderAttackSound` siren that follows an accepted
/// under-attack EVA line.
///
/// `HouseClass::NotifyUnderAttack`: the shared tail at
/// `0x004F95B8..0x004F95CF` loads `Rules+0x184` and plays it through
/// `VocClass::PlayAtPos @ 0x00750920` with pan `0x2000` and volume `1.0f`,
/// immediately after the EVA call at `0x004F95B3`. The ore-miner branch at
/// `0x004F94FB` plays its own EVA and then jumps (`0x004F9500 JMP`) to
/// `LAB_004F95D4`, past
/// the siren — so a harvester under attack is announced but not sirened.
/// Callers apply the `CreateRadarEvent @ 0x0065FA70` accept gate
/// (`0x004F95A5`) before reaching here.
fn base_under_attack_siren(
    miner: bool,
    rules: &crate::rules::ruleset::RuleSet,
) -> Option<GameSoundEvent> {
    if miner {
        return None;
    }
    let sound_id = rules
        .general
        .base_under_attack_sound
        .as_deref()
        .filter(|id| !id.is_empty())?;
    Some(GameSoundEvent::BaseUnderAttackSfx {
        sound_id: sound_id.to_string(),
    })
}

/// The Ready line `VoxClass::RemoveFromQueues @ 0x00752A40` drops after the
/// local player's launch: the nuclear missile's case 0 drops its own
/// (`0x006CDE16`, `EVA_NuclearMissileReady`), the Iron Curtain's case 1 its own
/// (`0x006CD060`, `EVA_IronCurtainReady`), the Lightning Storm's case 2 its
/// own (`0x006CCDAB`, `EVA_LightningStormReady`), the Chrono Warp's case 4 the
/// Chronosphere's (`0x006CCD2D`, `EVA_ChronosphereReady`), the Psychic
/// Dominator's case 7 its own (`0x006CCE52`, `EVA_PsychicDominatorReady`),
/// and cases 5, 6, 8, 9 and 11
/// theirs through the shared tail `0x006CD51E`: the paradrops
/// `EVA_ReinforcementsReady` (`0x006CD519`), the Spy Plane
/// `EVA_SpyPlaneReady` (`0x006CD702`), the Genetic Mutator
/// `EVA_GeneticMutatorReady` (`0x006CDA5D`), the Psychic Reveal
/// `EVA_PsychicRevealReady` (`0x006CD7DD`); the Force Shield's case 10 its own
/// (`0x006CD2C6`, `EVA_ForceShieldReady`). Each tail also clears the local
/// selection (`super_selection::follow_selection_writes`).
fn launch_drops_ready_line(
    kind: crate::rules::superweapon_type::SuperWeaponKind,
) -> Option<&'static str> {
    use crate::rules::superweapon_type::SuperWeaponKind as K;
    match kind {
        K::MultiMissile => Some("EVA_NuclearMissileReady"),
        K::IronCurtain => Some("EVA_IronCurtainReady"),
        K::LightningStorm => Some("EVA_LightningStormReady"),
        K::ChronoWarp => Some("EVA_ChronosphereReady"),
        K::PsychicDominator => Some("EVA_PsychicDominatorReady"),
        K::ParaDrop | K::AmerParaDrop => Some("EVA_ReinforcementsReady"),
        K::SpyPlane => Some("EVA_SpyPlaneReady"),
        K::GeneticConverter => Some("EVA_GeneticMutatorReady"),
        K::ForceShield => Some("EVA_ForceShieldReady"),
        K::PsychicReveal => Some("EVA_PsychicRevealReady"),
        _ => None,
    }
}

/// `HouseClass::IsHumanPlayer @ 0x0050B6F0` as the EVA sites use it: in a
/// multiplayer game it is `this == PlayerPtr`, the local player. The sim
/// carries house identity; the app holds the local name.
fn owner_is_local(
    interner: &crate::sim::intern::StringInterner,
    owner: crate::sim::intern::InternedId,
    local_owner_name: Option<&str>,
) -> bool {
    let owner_str = interner.resolve(owner);
    local_owner_name.is_some_and(|local| local.eq_ignore_ascii_case(owner_str))
}

/// The outcome line: `HouseClass::Flag_To_Win 0x004FCBA9` plays
/// `EVA_YouAreVictorious` (`0x00824C2C`) and `Flag_To_Lose 0x004FCDA1`
/// `EVA_YouHaveLost` (`0x00824C08`), both with `EDX = -1` (the entry's own
/// type; stock rows are STANDARD NORMAL). The side column is the consumer's.
fn outcome_eva_event(kind: crate::sim::house_state::HouseOutcomeKind) -> &'static str {
    match kind {
        crate::sim::house_state::HouseOutcomeKind::Victory => "EVA_YouAreVictorious",
        crate::sim::house_state::HouseOutcomeKind::Defeat => "EVA_YouHaveLost",
    }
}

fn anim_world_sound_source(world: crate::sim::anim_class::AnimWorldCoord) -> SoundSource {
    let (rx, ry, sub_x, sub_y, _) = world.to_cell_sub_z();
    let screen = crate::util::lepton::lepton_to_screen_exact_z(rx, ry, sub_x, sub_y, world.z);
    SoundSource::new(screen, (rx, ry))
}

/// What one superweapon `Type=` case plays when it fires.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct SuperWeaponLaunchCue {
    /// `[AudioVisual]` (or the type's own `StartSound=`) cue name, if any.
    pub sound_id: Option<String>,
    /// `true` when the cue plays at the target coordinate. Every launch cue
    /// this table still carries is positional; the one non-positional cue in
    /// the system, `StormSound`, is not a launch cue at all — see
    /// [`SimSoundEvent::LightningStormBegan`].
    pub positional: bool,
    /// `evamd.ini` event name, if the case plays one. Every site passes type
    /// `-1`, so the entry's own `Type=`/`Priority=` route it.
    pub eva_event: Option<&'static str>,
}

/// The launch cue table of `SuperClass::Launch @ 0x006CC390`.
///
/// Native switches on `*(param_1[10] + 0xB4)` — the launched
/// `SuperWeaponTypeClass`'s `Type=` index, whose name table is the 12 pointers
/// at `0x008425C0` (`MultiMissile`, `IronCurtain`, `LightningStorm`,
/// `ChronoSphere`, `ChronoWarp`, `ParaDrop`, `AmerParaDrop`,
/// `PsychicDominator`, `SpyPlane`, `GeneticConverter`, `ForceShield`,
/// `PsychicReveal`), i.e. exactly [`SuperWeaponKind`]'s order. Per case:
///
/// | `Type=` | cue | EVA |
/// |---|---|---|
/// | `MultiMissile` | `[Rules+0x174]` `DigSound` at target (`0x006CDCAE`, and `0x006CDDE9` on the silo branch) | `EVA_NuclearMissileLaunched` (`0x006CDC98`, `0x006CDE01`) |
/// | `IronCurtain` | — | `EVA_IronCurtainActivated` (`0x006CCF21`) |
/// | `LightningStorm` | — (the `StormSound` cue is deferred, see [`SimSoundEvent::LightningStormBegan`]) | `EVA_LightningStormCreated` (`0x006CCD81`) |
/// | `ChronoSphere` | — | — |
/// | `ChronoWarp` | — | `EVA_ChronosphereActivated` (`0x006CCD03`) |
/// | `ParaDrop`, `AmerParaDrop`, `SpyPlane` | — | — |
/// | `PsychicDominator` | `[Rules+0x24C]` at target (`0x006CCE28`) | `EVA_PsychicDominatorActivated` (`0x006CCDFA`) |
/// | `GeneticConverter` | `[Rules+0x250]` at target (`0x006CD8D3`) | `EVA_GeneticMutatorActivated` (`0x006CD8BD`) |
/// | `ForceShield` | the **type's own** `StartSound=` (`SuperWeaponTypeClass+0xC4`, key at `0x00818418`) at target, skipped when `-1` (`0x006CD16B CMP ECX,-1 ; JZ`), through `VocClass::PlayAt @ 0x007509E0` (`0x006CD176`) | — |
/// | `PsychicReveal` | `[Rules+0x254]` at target (`0x006CD7BF`) | — |
///
/// Every EVA call sits behind `MOV EAX,[0x00A8B538] ; TEST ; JNZ` and nothing
/// else — not the launching house — so an enemy launch announces itself too.
/// `0x00A8B538` is the client-side defeated/spectating flag
/// (`HouseClass::MPlayer_Defeated @ 0x004FC205` sets it to 1); VERA has no
/// equivalent state, so the gate is unmodelled and always open here.
pub(crate) fn superweapon_launch_cue(
    kind: crate::rules::superweapon_type::SuperWeaponKind,
    general: &crate::rules::ruleset::GeneralRules,
    start_sound: Option<&str>,
) -> SuperWeaponLaunchCue {
    use crate::rules::superweapon_type::SuperWeaponKind as K;
    let positional = SuperWeaponLaunchCue {
        positional: true,
        ..Default::default()
    };
    match kind {
        K::MultiMissile => SuperWeaponLaunchCue {
            sound_id: general.dig_sound.clone(),
            eva_event: Some("EVA_NuclearMissileLaunched"),
            ..positional
        },
        K::IronCurtain => SuperWeaponLaunchCue {
            eva_event: Some("EVA_IronCurtainActivated"),
            ..Default::default()
        },
        // Case 2 plays the EVA line and nothing else at launch. It hands
        // `[Rules+0x1794]` (`LightningDeferment`, stock 250) to
        // `LightningStorm::Start @ 0x00539EB0`, whose `if (param_2 != 0)`
        // early return fires before the `StormSound` call at `0x0053A044` —
        // so the cue belongs to the deferment expiry, not to the launch.
        // [`SimSoundEvent::LightningStormBegan`] carries it.
        K::LightningStorm => SuperWeaponLaunchCue {
            eva_event: Some("EVA_LightningStormCreated"),
            ..Default::default()
        },
        K::ChronoWarp => SuperWeaponLaunchCue {
            eva_event: Some("EVA_ChronosphereActivated"),
            ..Default::default()
        },
        K::PsychicDominator => SuperWeaponLaunchCue {
            sound_id: general.psychic_dominator_activate_sound.clone(),
            // `evamd.ini [EVA_PsychicDominatorActivated] Type=QUEUE` — and all
            // three of its faction values are `Dummy`, so on retail data the
            // line resolves to a zero-sample entry and is inaudible; only the
            // `[AudioVisual]` cue is heard.
            eva_event: Some("EVA_PsychicDominatorActivated"),
            ..positional
        },
        K::GeneticConverter => SuperWeaponLaunchCue {
            sound_id: general.genetic_mutator_activate_sound.clone(),
            // `evamd.ini [EVA_GeneticMutatorActivated] Type=QUEUE`.
            eva_event: Some("EVA_GeneticMutatorActivated"),
            ..positional
        },
        K::ForceShield => SuperWeaponLaunchCue {
            sound_id: start_sound
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            ..positional
        },
        K::PsychicReveal => SuperWeaponLaunchCue {
            sound_id: general.psychic_reveal_activate_sound.clone(),
            ..positional
        },
        // Cases 3, 5, 6 and 8 reach neither a `VocClass` nor a `VoxClass` call.
        K::ChronoSphere | K::ParaDrop | K::AmerParaDrop | K::SpyPlane => {
            SuperWeaponLaunchCue::default()
        }
    }
}

/// The lines a frame's Super events post, as CSF labels. Every client posts
/// `TXT_LIGHTNING_STORM` when a storm starts (`LightningStorm::Start @
/// 0x0053A067`) and `TXT_LIGHTNING_STORM_APPROACHING` at the countdown's
/// warnings (`LightningStorm::Process @ 0x0053AB31`). Only the client whose
/// player owns a refused Super posts ClickFire's line for it:
/// `Msg:LightningStormActive` (`LightningStorm::PrintMessage @ 0x0053AE00`)
/// or `Msg:DominatorActive` (`PsyDom::PrintMessage @ 0x0053B410`).
pub(super) fn super_weapon_messages(
    events: &[SimSoundEvent],
    interner: &crate::sim::intern::StringInterner,
    local_owner_name: Option<&str>,
) -> Vec<&'static str> {
    events
        .iter()
        .filter_map(|event| match *event {
            SimSoundEvent::LightningStormBegan => Some("TXT_LIGHTNING_STORM"),
            SimSoundEvent::LightningStormApproaching => Some("TXT_LIGHTNING_STORM_APPROACHING"),
            SimSoundEvent::LightningStormRefused { owner } => {
                owner_is_local(interner, owner, local_owner_name)
                    .then_some("Msg:LightningStormActive")
            }
            SimSoundEvent::PsychicDominatorRefused { owner } => {
                owner_is_local(interner, owner, local_owner_name).then_some("Msg:DominatorActive")
            }
            _ => None,
        })
        .collect()
}

/// The sound source of a flat cell event. Native `VocClass::PlayAt`
/// callers pass an object or cell coordinate — the cell centre `(cell << 8)
/// + 0x80` — so the projected point is the cell's diamond centre, not the
/// tile's north-west corner.
fn sound_source_at_cell(rx: u16, ry: u16) -> SoundSource {
    let screen = crate::app::input::camera::cell_centre_world_point(rx, ry, 0);
    SoundSource::new(screen, (rx, ry))
}

fn cloak_sound_for_app(
    sound_id: String,
    rx: u16,
    ry: u16,
    sub_x: crate::util::fixed_math::SimFixed,
    sub_y: crate::util::fixed_math::SimFixed,
    world_z_leptons: i32,
) -> GameSoundEvent {
    let (sx, sy) =
        crate::util::lepton::lepton_to_screen_exact_z(rx, ry, sub_x, sub_y, world_z_leptons);
    GameSoundEvent::CloakSound {
        sound_id,
        source: Some(SoundSource::new((sx, sy), (rx, ry))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::fixed_math::SimFixed;

    fn dispatch_rules() -> RuleSet {
        RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[General]\nFixtureOnly=1\n[InfantryTypes]\n0=E1\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [E1]\nVoiceFeedback=Feedback\n\
             [AudioVisual]\nBaseUnderAttackSound=Siren\n",
        ))
        .unwrap()
    }

    /// Original458200 calls global Voc before radar15 and speaks only when
    /// that radar request accepts. Simulation already admitted House50B6F0;
    /// a campaign human need not be the local-owner name used by other arms.
    #[test]
    fn building_abandoned_dispatch_matches_native_sound_radar_and_eva_order() {
        use crate::sim::radar::{RadarEventRequest, RadarEventType};
        use std::cell::RefCell;

        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/garrison_oracle/allegiance.json",
        ))
        .unwrap();
        assert_eq!(fixture["schema_version"], 1);
        let rules = dispatch_rules();
        for name in [
            "empty_player_abandons",
            "radar_refusal_suppresses_eva",
            "campaign_human_abandons",
            "campaign_player_control_abandons",
        ] {
            let row = fixture["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["input"]["name"] == name)
                .unwrap();
            let input = &row["input"];
            let calls = row["output"]["calls"].as_array().unwrap();
            let native_sound = calls
                .iter()
                .find(|call| call["op"] == "abandoned_sound")
                .unwrap();
            assert_eq!(native_sound["pan"], 0x2000);
            assert_eq!(native_sound["volume_bits"], "3f800000");
            assert_eq!(native_sound["handle"], 0);
            let native_radar = calls
                .iter()
                .find(|call| call["op"] == "radar_event")
                .unwrap();
            let request = RadarEventRequest::new(
                RadarEventType::StructureAbandoned,
                native_radar["cell"][0].as_u64().unwrap() as u16,
                native_radar["cell"][1].as_u64().unwrap() as u16,
            );
            assert_eq!(
                request.event_type as u8,
                native_radar["event"].as_u64().unwrap() as u8
            );
            let mut sim = Simulation::new();
            sim.session.game_mode_nonzero = input["game_mode"].as_u64().unwrap() != 0;
            let houses = input["houses"].as_array().unwrap();
            let owner_name = houses[input["current_owner"].as_u64().unwrap() as usize]["name"]
                .as_str()
                .unwrap();
            let local_name = houses[input["local_house"].as_u64().unwrap() as usize]["name"]
                .as_str()
                .unwrap();
            let owner = sim.interner.intern(owner_name);
            let sound_name = format!("NativeSound{}", native_sound["sound"].as_i64().unwrap());
            let events = [
                SimSoundEvent::VocCentered {
                    sound_id: sound_name.clone(),
                },
                SimSoundEvent::StructureAbandoned {
                    owner,
                    radar: request,
                },
            ];
            let observed = RefCell::new(Vec::new());
            let mut gate = |actual| {
                assert_eq!(actual, request, "{name}");
                observed.borrow_mut().push("radar_event");
                native_radar["accepted"].as_bool().unwrap()
            };
            let mut output = SoundEventQueue::new();
            // Observe each prepared producer event at the existing dispatcher
            // boundary, preserving one ledger across the sound and radar calls.
            // The producer's choice/order is replayed separately in sim tests.
            for event in events {
                dispatch_sim_sound_events(
                    [event],
                    &sim,
                    &rules,
                    Some(local_name),
                    &mut gate,
                    &mut output,
                );
                for emitted in output.drain() {
                    match emitted {
                        GameSoundEvent::VocAt {
                            sound_id,
                            source: None,
                        } => {
                            assert_eq!(sound_id, sound_name, "{name}");
                            observed.borrow_mut().push("abandoned_sound");
                        }
                        GameSoundEvent::Eva {
                            event,
                            type_override: None,
                        } => {
                            assert_eq!(event, "EVA_StructureAbandoned", "{name}");
                            observed.borrow_mut().push("abandoned_eva");
                        }
                        unexpected => panic!("{name}: unexpected event {unexpected:?}"),
                    }
                }
            }
            let expected = calls
                .iter()
                .map(|call| call["op"].as_str().unwrap())
                .filter(|op| matches!(*op, "abandoned_sound" | "radar_event" | "abandoned_eva"))
                .collect::<Vec<_>>();
            assert_eq!(*observed.borrow(), expected, "{name}");
        }
    }

    /// House4FB627..649 calls named EVA752700 only when client kind6
    /// insertion accepts. The actual client lifecycle is compared separately
    /// in radar_events_tests; this boundary receives those recorded returns.
    #[test]
    fn unit_ready_dispatch_preserves_native_client_gate_and_local_listener() {
        use crate::sim::radar::{RadarEventRequest, RadarEventType};

        let fixture = crate::rules::retail_ini_fixture::factory_unit_ready_native();
        let suffix = fixture["controls"]["cadence"]["registered_prior"]["notification_suffix"]
            .as_array()
            .unwrap();
        let native_voice_calls: Vec<bool> = suffix
            .iter()
            .map(|row| {
                row["original_call_order"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|pc| pc == "0x00752700")
            })
            .collect();
        let rules = dispatch_rules();
        let mut sim = Simulation::new();
        let local = sim.interner.intern("Local");
        let remote = sim.interner.intern("Remote");
        let request = RadarEventRequest::new(RadarEventType::UnitReady, 15, 15);
        let event = |owner| SimSoundEvent::UnitComplete {
            owner,
            radar: request,
        };
        let mut admitted = Vec::new();
        let mut gate = |actual| {
            assert_eq!(actual, request);
            let result = native_voice_calls[admitted.len()];
            admitted.push(actual);
            result
        };
        let mut output = SoundEventQueue::new();
        dispatch_sim_sound_events(
            [event(remote), event(local), event(local)],
            &sim,
            &rules,
            Some("LOCAL"),
            &mut gate,
            &mut output,
        );
        assert_eq!(admitted.len(), suffix.len());
        let emitted = output.drain();
        assert_eq!(
            emitted.len(),
            native_voice_calls.iter().filter(|called| **called).count()
        );
        let expected = suffix[0]["after"]["selected"][0]["name"].as_str().unwrap();
        assert!(matches!(
            &emitted[0],
            GameSoundEvent::Eva { event, type_override: None } if event == expected
        ));
    }

    /// `0x004FB2CC..0x004FB314`: BuildingSlam is the player's own cue, read
    /// from retail `[AudioVisual]`.
    #[test]
    fn building_slam_plays_only_for_the_local_owner() {
        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let rules = RuleSet::from_ini(&ini).expect("retail rules");
        let mut sim = Simulation::new();
        let local = sim.interner.intern("Local");
        let remote = sim.interner.intern("Remote");
        let mut output = SoundEventQueue::new();
        dispatch_sim_sound_events(
            [
                SimSoundEvent::BuildingPlaced { owner: remote },
                SimSoundEvent::BuildingPlaced { owner: local },
            ],
            &sim,
            &rules,
            Some("LOCAL"),
            &mut |_| true,
            &mut output,
        );
        let emitted = output.drain();
        assert_eq!(emitted.len(), 1);
        assert!(matches!(
            &emitted[0],
            GameSoundEvent::UiSound { sound_id } if sound_id == "PlaceBuilding"
        ));
    }

    #[test]
    fn dispatcher_preserves_batch_and_multi_cue_order_and_listener_gates() {
        let rules = dispatch_rules();
        let mut sim = Simulation::new();
        let local = sim.interner.intern("Local");
        let remote = sim.interner.intern("Remote");
        let sound = sim.interner.intern("Promotion");
        let promotion = |owner| SimSoundEvent::UnitPromoted {
            owner,
            sound_id: Some(sound),
            elite: true,
            rx: 5,
            ry: 7,
        };
        use crate::sim::radar::{RadarEventRequest, RadarEventType};
        // The scripted client radar refuses every request at column 99.
        let attack = |owner, miner, radar_accepts: bool| {
            let rx = if radar_accepts { 5 } else { 99 };
            let event_type = if miner {
                RadarEventType::HarvesterUnderAttack
            } else {
                RadarEventType::BaseUnderAttack
            };
            SimSoundEvent::UnderAttack {
                owner,
                miner,
                radar: RadarEventRequest::new(event_type, rx, 7),
                rx,
                ry: 7,
            }
        };
        let unit_lost = |owner, rx| SimSoundEvent::UnitLost {
            owner,
            radar: RadarEventRequest::new(RadarEventType::UnitLost, rx, 7),
        };
        let mut admitted = Vec::new();
        let mut admit_radar = |request: RadarEventRequest| {
            admitted.push((request.event_type, request.rx));
            request.rx != 99
        };
        let mut output = SoundEventQueue::new();
        output.push(GameSoundEvent::UiSound {
            sound_id: "Earlier".into(),
        });
        dispatch_sim_sound_events(
            [
                promotion(remote),
                promotion(local),
                attack(local, false, true),
                attack(remote, false, true),
                attack(local, false, false),
                attack(local, true, true),
                unit_lost(remote, 5),
                unit_lost(local, 99),
                unit_lost(local, 5),
            ],
            &sim,
            &rules,
            Some("LOCAL"),
            &mut admit_radar,
            &mut output,
        );
        assert_eq!(
            admitted,
            [
                (RadarEventType::BaseUnderAttack, 5),
                (RadarEventType::BaseUnderAttack, 99),
                (RadarEventType::HarvesterUnderAttack, 5),
                (RadarEventType::UnitLost, 99),
                (RadarEventType::UnitLost, 5),
            ],
            "only the local player's events reach its radar array, once each, in producer order"
        );
        let events = output.drain();
        assert_eq!(
            events
                .iter()
                .map(GameSoundEvent::sound_id)
                .collect::<Vec<_>>(),
            [
                "Earlier",
                "Promotion",
                "EVA_UnitPromoted",
                "EVA_OurBaseIsUnderAttack",
                "Siren",
                "EVA_OreMinerUnderAttack",
                "EVA_UnitLost"
            ]
        );
        assert_eq!(events[1].source().unwrap().cell(), (5, 7));
        assert!(matches!(
            events[4],
            GameSoundEvent::BaseUnderAttackSfx { .. }
        ));
    }

    #[test]
    fn depot_eva_uses_the_local_listener_and_existing_radar_admission() {
        use crate::sim::radar::{RadarEventRequest, RadarEventType};
        let rules = dispatch_rules();
        let mut sim = Simulation::new();
        let local = sim.interner.intern("Local");
        let remote = sim.interner.intern("Remote");
        let repaired = |owner, rx| SimSoundEvent::UnitRepaired {
            owner,
            radar: RadarEventRequest::new(RadarEventType::UnitRepaired, rx, 7),
        };
        let mut admitted = Vec::new();
        let mut output = SoundEventQueue::new();
        dispatch_sim_sound_events(
            [
                repaired(remote, 5),
                SimSoundEvent::Repairing { owner: local },
                SimSoundEvent::HouseEva {
                    owner: local,
                    event: "EVA_InsufficientFunds",
                },
                repaired(local, 99),
                repaired(local, 5),
            ],
            &sim,
            &rules,
            Some("LOCAL"),
            &mut |request| {
                admitted.push((request.event_type, request.rx));
                request.rx != 99
            },
            &mut output,
        );
        assert_eq!(
            admitted,
            [
                (RadarEventType::UnitRepaired, 99),
                (RadarEventType::UnitRepaired, 5)
            ]
        );
        assert_eq!(
            output
                .drain()
                .iter()
                .map(GameSoundEvent::sound_id)
                .collect::<Vec<_>>(),
            ["EVA_Repairing", "EVA_InsufficientFunds", "EVA_UnitRepaired"],
        );
    }

    #[test]
    fn dispatcher_preserves_resolved_feedback_before_the_building_cue() {
        let mut rules = dispatch_rules();
        rules.general.building_damage_sound = Some("Damaged".into());
        let sim = Simulation::new();
        let before = sim.rng_state();
        let mut output = SoundEventQueue::new();
        dispatch_sim_sound_events(
            [
                SimSoundEvent::VocAt {
                    sound_id: "Feedback".into(),
                    audible_to: None,
                    rx: 3,
                    ry: 4,
                    sub_x: SimFixed::from_num(61),
                    sub_y: SimFixed::from_num(174),
                    world_z_leptons: 104,
                },
                SimSoundEvent::BuildingDamagedSfx { rx: 3, ry: 4 },
            ],
            &sim,
            &rules,
            Some("DifferentListener"),
            &mut |_| panic!("these cues have no radar gate"),
            &mut output,
        );
        let events = output.drain();
        assert_eq!(
            events
                .iter()
                .map(GameSoundEvent::sound_id)
                .collect::<Vec<_>>(),
            ["Feedback", "Damaged"]
        );
        let source = events[0].source().unwrap();
        assert_eq!(source.cell(), (3, 4));
        assert_eq!(
            source.screen_pos(),
            crate::util::lepton::lepton_to_screen_exact_z(
                3,
                4,
                SimFixed::from_num(61),
                SimFixed::from_num(174),
                104,
            )
        );
        assert_eq!(
            sim.rng_state(),
            before,
            "the app only forwards resolved requests"
        );
    }

    #[test]
    fn centered_voc_preserves_order_without_listener_or_random_gates() {
        // Walk75B085..75B0B0 calls 750920 with centre pan, volume1 and no handle.
        // The existing registered Voc consumer maps source=None to those
        // gains without a positional shroud/distance test.
        let rules = dispatch_rules();
        let sim = Simulation::new();
        for local_owner in [None, Some("DifferentOwner")] {
            let mut output = SoundEventQueue::new();
            dispatch_sim_sound_events(
                [
                    SimSoundEvent::VocCentered {
                        sound_id: "MenuScold".into(),
                    },
                    SimSoundEvent::VocCentered {
                        sound_id: "OtherRulesSound".into(),
                    },
                ],
                &sim,
                &rules,
                local_owner,
                &mut |_| panic!("centred Voc has no radar admission"),
                &mut output,
            );
            let events = output.drain();
            assert!(matches!(
                events.as_slice(),
                [
                    GameSoundEvent::VocAt { sound_id: first, source: None },
                    GameSoundEvent::VocAt { sound_id: second, source: None },
                ] if first == "MenuScold" && second == "OtherRulesSound"
            ));
        }
    }

    /// Launch case 4's tail (`0x006CCCF0..0x006CCD2D`): `PlayEVA`
    /// (`EVA_ChronosphereActivated`) for every launcher, then for the
    /// launching player only `RemoveFromQueues(EVA_ChronosphereReady)`; its
    /// two radar events reach the client's radar and play nothing.
    #[test]
    fn launch_drops_the_ready_line_for_its_player_only() {
        for (name, activated, ready) in [
            (
                "ChronoWarpSpecial",
                "EVA_ChronosphereActivated",
                "EVA_ChronosphereReady",
            ),
            (
                "PsychicDominatorSpecial",
                "EVA_PsychicDominatorActivated",
                "EVA_PsychicDominatorReady",
            ),
        ] {
            launch_drops_the_ready_line(name, activated, ready);
        }
    }

    fn launch_drops_the_ready_line(name: &str, activated: &str, ready: &str) {
        use crate::sim::radar::{RadarEventRequest, RadarEventType};

        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[General]\nFixtureOnly=1\n[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
                 [BuildingTypes]\n[SuperWeaponTypes]\n0=ChronoWarpSpecial\n\
                 1=PsychicDominatorSpecial\n[ChronoWarpSpecial]\nType=ChronoWarp\n\
                 [PsychicDominatorSpecial]\nType=PsychicDominator\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        let local = sim.interner.intern("Local");
        let remote = sim.interner.intern("Remote");
        let sw_type = sim.interner.intern(name);
        let launched = |owner| SimSoundEvent::SuperWeaponLaunched {
            owner,
            sw_type,
            rx: 40,
            ry: 40,
        };
        let radar = |rx, ry| SimSoundEvent::SuperWeaponRadarEvent {
            radar: RadarEventRequest::new(RadarEventType::ImpactSilent, rx, ry),
        };
        let mut admitted = Vec::new();
        let mut output = SoundEventQueue::new();
        dispatch_sim_sound_events(
            [
                radar(21, 21),
                radar(40, 40),
                launched(remote),
                launched(local),
            ],
            &sim,
            &rules,
            Some("LOCAL"),
            &mut |request| {
                admitted.push((request.rx, request.ry));
                true
            },
            &mut output,
        );
        assert_eq!(admitted, [(21, 21), (40, 40)]);
        let emitted = output.drain();
        assert!(
            matches!(
                emitted.as_slice(),
                [
                    GameSoundEvent::SuperWeaponActivated { eva_event: Some(remote_line), .. },
                    GameSoundEvent::SuperWeaponActivated { eva_event: Some(local_line), .. },
                    GameSoundEvent::EvaRemove { event },
                ] if remote_line == activated
                    && local_line == activated
                    && event == ready
            ),
            "{emitted:?}"
        );
    }

    /// Cases 0, 1, 9 and 11's lines against `tools/superweapon_oracle.json`
    /// `nuke_launch`, `iron_curtain_launch`, `genetic_launch` and
    /// `psychic_launch`: each launching row plays `DigSound=` (`0x006CDDE9`)
    /// and `EVA_NuclearMissileLaunched` (`0x006CDE01`), or
    /// `EVA_IronCurtainActivated` (`0x006CCF21`), or
    /// `EVA_GeneticMutatorActivated` (`0x006CD8BD`) and
    /// `GeneticMutatorActivateSound=` (`0x006CD8D3`), or only
    /// `PsychicRevealActivateSound=` (`0x006CD7BF`); the player's then drops
    /// the queued `EVA_NuclearMissileReady` (`0x006CDE16`),
    /// `EVA_IronCurtainReady` (`0x006CD04A..0x006CD060`),
    /// `EVA_GeneticMutatorReady` (`0x006CDA5D`) or `EVA_PsychicRevealReady`
    /// (`0x006CD7DD`), the last two through the shared `0x006CD51E`. One
    /// launch event carries a case's sound and line, so case 0's order, its
    /// sound first, is not compared. Left out: the mute rows (`0x00A8B538`;
    /// VERA has no such byte, the launches' RESIDUAL). The player's charged
    /// case 0 rows that find no silo pin the `nuke.rs` RESIDUAL: native drops
    /// the Ready line, VERA runs no tail.
    #[test]
    fn launch_lines_match_native() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[General]\nFixtureOnly=1\n[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
                 [BuildingTypes]\n[SuperWeaponTypes]\n0=IronCurtainSpecial\n\
                 1=GeneticConverterSpecial\n2=PsychicRevealSpecial\n3=NukeSpecial\n\
                 [IronCurtainSpecial]\nType=IronCurtain\n\
                 [GeneticConverterSpecial]\nType=GeneticConverter\n\
                 [PsychicRevealSpecial]\nType=PsychicReveal\n\
                 [NukeSpecial]\nType=MultiMissile\n\
                 [AudioVisual]\nGeneticMutatorActivateSound=GeneticMutatorActivate\n\
                 PsychicRevealActivateSound=PsychicRevealActivate\nDigSound=NukeSiren\n",
            ))
            .unwrap();
        let oracle: serde_json::Value =
            serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json"))
                .unwrap();
        for (section, name, sound, expected) in [
            ("nuke_launch", "NukeSpecial", "NukeSiren", 11),
            ("iron_curtain_launch", "IronCurtainSpecial", "", 17),
            (
                "genetic_launch",
                "GeneticConverterSpecial",
                "GeneticMutatorActivate",
                16,
            ),
            (
                "psychic_launch",
                "PsychicRevealSpecial",
                "PsychicRevealActivate",
                11,
            ),
        ] {
            let mut compared = 0;
            for row in oracle[section].as_array().unwrap() {
                if row["mute"] == true {
                    continue;
                }
                let events = row["events"].as_array().unwrap();
                // Case 0 launches only with a silo to fly the missile.
                let launches = row["charged"] == true
                    && (section != "nuke_launch"
                        || events.iter().any(|event| event[0] == "queue_mission"));
                // nuke.rs RESIDUAL: a charged player's launch without one
                // still drops the Ready line natively; VERA reports nothing.
                let residual = row["charged"] == true && !launches && row["player"] == true;
                let mut sim = Simulation::new();
                let owner = sim.interner.intern(if row["player"] == true {
                    "Local"
                } else {
                    "Remote"
                });
                let sw_type = sim.interner.intern(name);
                let launched = launches.then_some(SimSoundEvent::SuperWeaponLaunched {
                    owner,
                    sw_type,
                    rx: 40,
                    ry: 40,
                });
                let mut output = SoundEventQueue::new();
                dispatch_sim_sound_events(
                    launched,
                    &sim,
                    &rules,
                    Some("LOCAL"),
                    &mut |_| true,
                    &mut output,
                );
                let mut lines: Vec<String> = output
                    .drain()
                    .into_iter()
                    .flat_map(|event| match event {
                        GameSoundEvent::SuperWeaponActivated {
                            eva_event,
                            sound_id,
                            ..
                        } => eva_event
                            .map(|line| format!("eva {line}"))
                            .into_iter()
                            .chain((!sound_id.is_empty()).then(|| format!("play {sound_id}")))
                            .collect::<Vec<_>>(),
                        GameSoundEvent::EvaRemove { event } => vec![format!("remove {event}")],
                        other => panic!("{other:?}"),
                    })
                    .collect();
                let mut native = Vec::new();
                for event in events {
                    match event[0].as_str().unwrap() {
                        "eva" => native.push(format!("eva {}", event[1].as_str().unwrap())),
                        "play_at" => native.push(format!("play {sound}")),
                        "vox_find" => native.push(format!("remove {}", event[1].as_str().unwrap())),
                        _ => {}
                    }
                }
                if section == "nuke_launch" {
                    // Case 0 plays DigSound before its line (`0x006CDDE9`,
                    // `0x006CDE01`); the one launch event carries both.
                    lines.sort();
                    native.sort();
                }
                if residual {
                    assert!(lines.is_empty(), "{row}");
                    assert_eq!(native, ["remove EVA_NuclearMissileReady"], "{row}");
                } else {
                    assert_eq!(lines, native, "{row}");
                }
                compared += 1;
            }
            assert_eq!(compared, expected, "{section}");
        }
    }

    /// Cases 5, 6 and 8 play nothing, but their player's launch still drops
    /// the queued Ready line through the shared tail `0x006CD51E`.
    #[test]
    fn silent_launches_drop_their_ready_line_for_their_player_only() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[General]\nFixtureOnly=1\n[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
                 [BuildingTypes]\n[SuperWeaponTypes]\n0=ParaDropSpecial\n\
                 1=AmericanParaDropSpecial\n2=SpyPlaneSpecial\n[ParaDropSpecial]\n\
                 Type=ParaDrop\n[AmericanParaDropSpecial]\nType=AmerParaDrop\n\
                 [SpyPlaneSpecial]\nType=SpyPlane\n",
            ))
            .unwrap();
        for (name, ready) in [
            ("ParaDropSpecial", "EVA_ReinforcementsReady"),
            ("AmericanParaDropSpecial", "EVA_ReinforcementsReady"),
            ("SpyPlaneSpecial", "EVA_SpyPlaneReady"),
        ] {
            let mut sim = Simulation::new();
            let local = sim.interner.intern("Local");
            let remote = sim.interner.intern("Remote");
            let sw_type = sim.interner.intern(name);
            let launched = |owner| SimSoundEvent::SuperWeaponLaunched {
                owner,
                sw_type,
                rx: 40,
                ry: 40,
            };
            let mut output = SoundEventQueue::new();
            dispatch_sim_sound_events(
                [launched(remote), launched(local)],
                &sim,
                &rules,
                Some("LOCAL"),
                &mut |_| true,
                &mut output,
            );
            let emitted = output.drain();
            assert!(
                matches!(
                    emitted.as_slice(),
                    [GameSoundEvent::EvaRemove { event }] if event == ready
                ),
                "{name}: {emitted:?}"
            );
        }
    }

    #[test]
    fn gsi_01_04_outcome_transition_resolves_exact_standard_eva_entries() {
        use crate::sim::house_state::HouseOutcomeKind;

        assert_eq!(
            outcome_eva_event(HouseOutcomeKind::Victory),
            "EVA_YouAreVictorious"
        );
        assert_eq!(
            outcome_eva_event(HouseOutcomeKind::Defeat),
            "EVA_YouHaveLost"
        );
    }

    #[test]
    fn gsi_04_07_wall_sell_sound_is_global_only_for_local_receiver() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[General]\nFixtureOnly=1\n[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
                 [AudioVisual]\nSellSound=SellBuilding\n",
            ))
            .unwrap();
        let mut sim = crate::sim::world::Simulation::new();
        let receiver = sim.interner.intern("Receiver");
        let receiver_name = sim.interner.resolve(receiver);

        assert!(matches!(
            wall_sell_sound_for_local(receiver_name, Some("receiver"), Some(&rules)),
            Some(crate::audio::events::GameSoundEvent::UiSound { sound_id })
                if sound_id == "SellBuilding"
        ));
        assert!(
            wall_sell_sound_for_local(receiver_name, Some("WallOwner"), Some(&rules)).is_none()
        );
        assert!(wall_sell_sound_for_local(receiver_name, None, Some(&rules)).is_none());

        let no_sound =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n",
            ))
            .unwrap();
        assert!(
            wall_sell_sound_for_local(receiver_name, Some("Receiver"), Some(&no_sound)).is_none()
        );
    }

    /// `HouseClass::NotifyUnderAttack`: the base siren rides the shared tail
    /// (`0x004F95CF`), and the ore-miner branch (EVA call `0x004F94FB`,
    /// `JMP` at `0x004F9500`) jumps past it.
    #[test]
    fn base_under_attack_siren_skips_the_ore_miner_branch() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[General]\nFixtureOnly=1\n[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
                 [BuildingTypes]\n[AudioVisual]\nBaseUnderAttackSound=BaseUnderAttackSiren\n",
            ))
            .unwrap();

        assert!(matches!(
            base_under_attack_siren(false, &rules),
            Some(crate::audio::events::GameSoundEvent::BaseUnderAttackSfx { sound_id })
                if sound_id == "BaseUnderAttackSiren"
        ));
        assert!(
            base_under_attack_siren(true, &rules).is_none(),
            "0x004F9500 jumps straight to LAB_004F95D4, so a harvester gets no siren"
        );

        let no_key =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[General]\nFixtureOnly=1\n[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
                 [BuildingTypes]\n",
            ))
            .unwrap();
        assert!(base_under_attack_siren(false, &no_key).is_none());
    }

    /// Stock `[AudioVisual]` values, so the table below is what a retail
    /// skirmish actually plays.
    fn stock_audio_visual() -> crate::rules::ruleset::GeneralRules {
        // The parse of each key from stock `[AudioVisual]` text is pinned in
        // `rules::ruleset::tests::superweapon_activate_sounds_parse_their_stock_values`.
        let mut general = crate::rules::ruleset::GeneralRules::default();
        general.psychic_dominator_activate_sound = Some("PsychicDominatorActivate".to_string());
        general.genetic_mutator_activate_sound = Some("GeneticMutatorActivate".to_string());
        general.psychic_reveal_activate_sound = Some("PsychicRevealActivate".to_string());
        general.dig_sound = Some("NukeSiren".to_string());
        general.storm_sound = Some("WeatherIntro".to_string());
        general
    }

    /// The whole `SuperClass::Launch @ 0x006CC390` cue table, one row per
    /// `Type=` case. Addresses are on `superweapon_launch_cue`.
    #[test]
    fn every_superweapon_case_plays_the_cue_its_native_arm_plays() {
        use crate::rules::superweapon_type::SuperWeaponKind as K;
        let g = stock_audio_visual();
        let cue = |kind, start: Option<&str>| superweapon_launch_cue(kind, &g, start);

        // Case 0: DigSound at the target plus the nuke EVA.
        let nuke = cue(K::MultiMissile, None);
        assert_eq!(nuke.sound_id.as_deref(), Some("NukeSiren"));
        assert!(nuke.positional);
        assert_eq!(nuke.eva_event, Some("EVA_NuclearMissileLaunched"));

        // Case 1: EVA only — the arm reaches no VocClass call.
        let ic = cue(K::IronCurtain, None);
        assert_eq!(ic.sound_id, None);
        assert_eq!(ic.eva_event, Some("EVA_IronCurtainActivated"));

        // Case 2: EVA only at launch. `Start` gets `LightningDeferment` as
        // `param_2` and returns before the cue, so `StormSound` is not a
        // launch cue — `the_storm_plays_its_cue_and_lines_where_the_sim_reports_them`
        // owns it.
        let storm = cue(K::LightningStorm, None);
        assert_eq!(
            storm.sound_id, None,
            "case 2 reaches no VocClass call of its own"
        );
        assert_eq!(storm.eva_event, Some("EVA_LightningStormCreated"));

        // Case 4 speaks; case 3 is entirely silent.
        assert_eq!(
            cue(K::ChronoWarp, None).eva_event,
            Some("EVA_ChronosphereActivated")
        );
        assert_eq!(cue(K::ChronoSphere, None), SuperWeaponLaunchCue::default());

        // Cases 5, 6 and 8 are silent: no VocClass and no VoxClass call.
        for kind in [K::ParaDrop, K::AmerParaDrop, K::SpyPlane] {
            assert_eq!(
                cue(kind, None),
                SuperWeaponLaunchCue::default(),
                "{kind:?} plays nothing in gamemd"
            );
        }

        // Cases 7 and 9: positional cue plus an EVA line (the entry's own
        // `Type=QUEUE` routes it; the cue table carries only the name).
        let dominator = cue(K::PsychicDominator, None);
        assert_eq!(
            dominator.sound_id.as_deref(),
            Some("PsychicDominatorActivate")
        );
        assert!(dominator.positional);
        assert_eq!(dominator.eva_event, Some("EVA_PsychicDominatorActivated"));
        let mutator = cue(K::GeneticConverter, None);
        assert_eq!(mutator.sound_id.as_deref(), Some("GeneticMutatorActivate"));
        assert!(mutator.positional);
        assert_eq!(mutator.eva_event, Some("EVA_GeneticMutatorActivated"));

        // Case 11: cue only, no EVA.
        let reveal = cue(K::PsychicReveal, None);
        assert_eq!(reveal.sound_id.as_deref(), Some("PsychicRevealActivate"));
        assert_eq!(reveal.eva_event, None);
    }

    /// Case 10 is the only one whose cue comes off the *type* object
    /// (`SuperWeaponTypeClass+0xC4`, `StartSound=`), and `0x006CD16B CMP
    /// ECX,-1 ; JZ 0x006CD17B` skips it when the type authors none. Stock
    /// `[ForceShieldSpecial]` is the only `[SuperWeaponTypes]` section that
    /// does author it.
    #[test]
    fn force_shield_takes_its_cue_from_the_launched_type_not_from_audiovisual() {
        use crate::rules::superweapon_type::SuperWeaponKind as K;
        let g = stock_audio_visual();

        let stock = superweapon_launch_cue(K::ForceShield, &g, Some("ForceShieldStarting"));
        assert_eq!(stock.sound_id.as_deref(), Some("ForceShieldStarting"));
        assert!(stock.positional);
        assert_eq!(stock.eva_event, None, "case 10 reaches no VoxClass call");

        let unset = superweapon_launch_cue(K::ForceShield, &g, None);
        assert_eq!(unset.sound_id, None);
        let empty = superweapon_launch_cue(K::ForceShield, &g, Some("  "));
        assert_eq!(empty.sound_id, None);
    }

    /// ClickFire's refusal lines (`LightningStorm::PrintMessage @
    /// 0x0053AE00`, `PsyDom::PrintMessage @ 0x0053B410`) post only on the
    /// client whose player owns the refused Super (Fire_SW passes `this ==
    /// PlayerPtr`, `0x004FAE8E`); the dispatch plays nothing for them, as
    /// the line's AddMessage plays IncomingMessage.
    #[test]
    fn a_refused_super_tells_only_its_own_player() {
        let mut sim = Simulation::new();
        let local = sim.interner.intern("Local");
        let other = sim.interner.intern("Other");
        let events = [
            SimSoundEvent::LightningStormRefused { owner: local },
            SimSoundEvent::PsychicDominatorRefused { owner: other },
            SimSoundEvent::PsychicDominatorRefused { owner: local },
            SimSoundEvent::LightningStormRefused { owner: other },
        ];
        assert_eq!(
            super_weapon_messages(&events, &sim.interner, Some("Local")),
            ["Msg:LightningStormActive", "Msg:DominatorActive"]
        );
        assert!(super_weapon_messages(&events, &sim.interner, None).is_empty());
        let mut output = SoundEventQueue::new();
        dispatch_sim_sound_events(
            events.clone(),
            &sim,
            &dispatch_rules(),
            Some("Local"),
            &mut |_| panic!("a refusal is not a radar event"),
            &mut output,
        );
        assert!(output.drain().is_empty());
    }

    /// `StormSound` belongs to the storm *beginning*, not to the launch.
    /// `SuperClass::Launch` case 2 hands `[Rules+0x1794]`
    /// (`LightningDeferment`, stock 250) to `LightningStorm::Start @
    /// 0x00539EB0`, whose deferred branch returns before the cue at
    /// `0x0053A044`; the countdown's end re-enters it (`0x0053AACA`). The sim
    /// reports that start and each countdown warning only under `[General]
    /// LightningPrintText=` (`lightning_storm_tests`); the launch's EVA line
    /// is outside that gate (`0x006CCD81`).
    #[test]
    fn the_storm_plays_its_cue_and_lines_where_the_sim_reports_them() {
        use crate::rules::superweapon_type::SuperWeaponKind as K;
        let mut rules = dispatch_rules();
        rules.general.storm_sound = Some("WeatherIntro".to_string());
        rules.general.lightning_print_text = false;
        assert_eq!(
            superweapon_launch_cue(K::LightningStorm, &rules.general, None),
            SuperWeaponLaunchCue {
                eva_event: Some("EVA_LightningStormCreated"),
                ..Default::default()
            },
            "the launch speaks with or without LightningPrintText"
        );

        let sim = Simulation::new();
        let events = [
            SimSoundEvent::LightningStormBegan,
            SimSoundEvent::LightningStormApproaching,
        ];
        assert_eq!(
            super_weapon_messages(&events, &sim.interner, None),
            ["TXT_LIGHTNING_STORM", "TXT_LIGHTNING_STORM_APPROACHING"]
        );
        let mut output = SoundEventQueue::new();
        for local in [Some("Local"), None] {
            dispatch_sim_sound_events(
                events.clone(),
                &sim,
                &rules,
                local,
                &mut |_| panic!("the storm's lines are not radar events"),
                &mut output,
            );
        }
        let played = output.drain();
        assert!(
            matches!(
                &played[..],
                [
                    GameSoundEvent::SuperWeaponActivated {
                        sound_id,
                        source: None,
                        eva_event: None,
                    },
                    GameSoundEvent::Eva {
                        event,
                        type_override: None,
                    },
                    GameSoundEvent::SuperWeaponActivated { .. },
                ] if sound_id == "WeatherIntro" && event == "EVA_LightningStormCreated"
            ),
            "the cue plays centred on every client, the EVA line with a local \
             player: {played:?}"
        );
    }

    #[test]
    fn cloak_sound_app_event_preserves_rule_identity_and_native_world_position() {
        let event = cloak_sound_for_app(
            "NavalUnitEmerge".to_string(),
            10,
            11,
            SimFixed::from_num(64),
            SimFixed::from_num(192),
            208,
        );
        assert!(matches!(
            event,
            crate::audio::events::GameSoundEvent::CloakSound {
                sound_id,
                source: Some(source),
            } if sound_id == "NavalUnitEmerge"
                && source.screen_pos() == (-45.0, 315.0)
                && source.cell() == (10, 11)
        ));
    }
}
