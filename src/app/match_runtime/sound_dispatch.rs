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

#[cfg(test)]
#[path = "sound_boundary_tests.rs"]
mod sound_boundary_tests;

#[cfg(test)]
#[path = "building_death_sound_tests.rs"]
mod building_death_sound_tests;

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
    let sound_id =
        sim.order_voice_request(rules, entity_id, "VoiceSpecialAttack", None, voices_enabled)?;
    Some(GameSoundEvent::UnitMoveOrder {
        speaker_id: entity_id,
        sound_id: sound_id.to_string(),
    })
}

/// ClickedMission's Unload arm (`0x006FFD3A`): VoiceDeploy through its one
/// owner (`TechnoClass::VoiceDeploy @ 0x00708E00`, the Building override
/// `0x00459C20` included). A Building's garrison or absorber self-unload
/// reaches it too (`BuildingClass::ClickedAction_Object @ 0x004434F2..
/// 0x00443562`); its factory arm queues an event instead and stays silent.
pub(crate) fn deploy_order_voice_event(
    sim: &mut Simulation,
    rules: &RuleSet,
    entity_id: u64,
    voices_enabled: bool,
) -> Option<GameSoundEvent> {
    // ClickedMission tests the selection-voice latch before it calls
    // VoiceDeploy, so muted voices leave Techno+0x4F8 alone.
    if !voices_enabled {
        return None;
    }
    let sound_id = sim.deploy_voice_request(rules, entity_id, true)?;
    Some(GameSoundEvent::UnitVoiceRequested {
        speaker_id: entity_id,
        sound_id: sound_id.to_string(),
    })
}

/// Collect one VERA-only accessibility row on an arm whose EVA voice has
/// already been admitted. `label` selects the VERA i18n template; `type_id`
/// is the sim's resolved object type, never a lookup or a guess. A disabled
/// switch passes `None`, so no row and no name lookup follows.
fn collect_eva_object(
    hud_text: &mut Option<&mut Vec<crate::app::input::messages::HudMessage>>,
    label: &'static str,
    type_id: crate::sim::intern::InternedId,
) {
    if let Some(messages) = hud_text.as_deref_mut() {
        messages.push(crate::app::input::messages::HudMessage::EvaObjectLine { label, type_id });
    }
}

/// Append each already-admitted event in producer order, including multi-cue
/// events. Simulation owns feedback admission and its Main draws at damage
/// time; this adapter never advances a random stream.
///
/// `admit_radar` is the local client's `CreateRadarEvent @ 0x0065FA70`. Caller
/// admission can already belong to the producer: `StructureAbandoned` carries
/// the simulation's `House50B6F0` admission. Other arms test the owner before
/// calling `admit_radar`: an arm whose native sites call
/// `HouseClass::IsHumanPlayer @ 0x0050B6F0` asks the sim's port
/// ([`Simulation::house_is_human_player`]), which in a campaign also accepts a
/// `PlayerControl=` house such as ALL01's Alliance; an arm whose sites compare
/// PlayerPtr asks [`owner_is_local`]. Each reached request calls `admit_radar`
/// at most once, in producer order; its result gates the caller's EVA line.
/// That ordering belongs to the caller, not to radar events.
///
/// RESIDUAL: four events are fed by both kinds of site and keep the PlayerPtr
/// test: CannotDeployHere (IsHumanPlayer in `UnitClass::Deploy` `0x007394E6`,
/// after the `DeploysInto=` building refuses the cell), UnderAttack (building
/// arm `0x004F9407`), HouseEva (Robot Tank, GoOnline / GoOffline, crate
/// upgrades) and Repairing (ToggleRepair `0x004470A4`). Trigger: a campaign
/// `PlayerControl=` house
/// that is not the player owns a building, a `DeploysInto=` unit or a Robot
/// Tank, or picks up a crate; effect: that line stays silent. No retail
/// campaign reaches it. Only ALL01 (Alliance: two ORCA teams; British:
/// nothing), ALL03 (Alliance: nothing) and SOV04 (Germans: RMNV infantry and a
/// SHAD transport with no `DeploysInto=`) have such a house; no map gives one
/// an object by placement, TeamType, Change House (actions 14 and 36) or
/// Create Building (125). Of these maps only ALL03 places crates (action 108).
///
/// `hud_text` collects VERA's optional detailed EVA text rows on the same
/// arms that admit the voice, so text and voice share every gate; `None`
/// (the switch off, tests, child bridges) builds no row.
pub(super) fn dispatch_sim_sound_events(
    events: impl IntoIterator<Item = SimSoundEvent>,
    sim: &Simulation,
    rules: &RuleSet,
    local_owner_name: Option<&str>,
    admit_radar: &mut dyn FnMut(crate::sim::radar::RadarEventRequest) -> bool,
    output: &mut SoundEventQueue,
    mut hud_text: Option<&mut Vec<crate::app::input::messages::HudMessage>>,
) {
    // Convert sim sound events to app-layer sound events for playback.
    for sim_event in events {
        let app_event: GameSoundEvent = match sim_event {
            SimSoundEvent::PlanningNodeAdded { owner } => {
                // PlanMgr638120 accepts the node before producing this cue.
                // Issuing bytes alone cannot play it: admission may reject them.
                if !owner_is_local(sim.interner(), owner, local_owner_name) {
                    continue;
                }
                let Some(sound_id) = rules.general.add_planning_mode_command_sound.clone() else {
                    continue;
                };
                GameSoundEvent::UiSound { sound_id }
            }
            SimSoundEvent::TriggerEva { index } => GameSoundEvent::TriggerEva { index },
            SimSoundEvent::VocCenteredIndex { index } => GameSoundEvent::VocIndex { index },
            SimSoundEvent::UnitVoiceVisit { owner } => GameSoundEvent::UnitVoiceVisit { owner },
            SimSoundEvent::UnitVoiceRequested { owner, sound_id } => {
                GameSoundEvent::UnitVoiceRequested {
                    speaker_id: owner,
                    sound_id,
                }
            }
            SimSoundEvent::UnitVoiceDestroyed { owner } => {
                GameSoundEvent::UnitVoiceDestroyed { owner }
            }
            SimSoundEvent::UnitVoiceMoved { from, to } => {
                GameSoundEvent::UnitVoiceMoved { from, to }
            }
            SimSoundEvent::ObjectSoundStarted {
                owner,
                sound_id,
                world,
            } => GameSoundEvent::AnimationStarted {
                anim_id: owner,
                sound_id: sim.interner().resolve(sound_id).to_string(),
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
                stop_sound_id: stop_sound_id.map(|id| sim.interner().resolve(id).to_string()),
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
                sound_id: sim.interner().resolve(sound_id).to_string(),
                source: Some(anim_world_sound_source(world)),
            },
            SimSoundEvent::EntityDied {
                die_sound_id,
                world,
            } => GameSoundEvent::EntityDestroyed {
                sound_id: die_sound_id,
                source: Some(anim_world_sound_source(world)),
            },
            SimSoundEvent::EntityCrushed {
                crush_sound_id,
                world,
            } => GameSoundEvent::EntityCrushed {
                sound_id: sim.interner().resolve(crush_sound_id).to_string(),
                source: Some(anim_world_sound_source(world)),
            },
            SimSoundEvent::EntityDeployed {
                deploy_sound_id,
                world,
            } => GameSoundEvent::EntityDeployed {
                sound_id: sim.interner().resolve(deploy_sound_id).to_string(),
                source: Some(anim_world_sound_source(world)),
            },
            SimSoundEvent::EntityUndeployed {
                undeploy_sound_id,
                world,
            } => GameSoundEvent::EntityUndeployed {
                sound_id: sim.interner().resolve(undeploy_sound_id).to_string(),
                source: Some(anim_world_sound_source(world)),
            },
            SimSoundEvent::ChronoTeleport { sound_id, world } => GameSoundEvent::ChronoTeleport {
                sound_id: sim.interner().resolve(sound_id).to_string(),
                source: Some(anim_world_sound_source(world)),
            },
            SimSoundEvent::UnitPromoted {
                owner,
                sound_id,
                type_id,
                world,
                #[cfg(feature = "ares")]
                eva,
            } => {
                // `HouseClass::IsHumanPlayer @ 0x0050B6F0` (`0x006FA089` /
                // `0x006FA0F6`): only a human player's promotion is audible.
                if !sim.house_is_human_player(owner) {
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
                        sound_id: sim.interner().resolve(sound_id).to_string(),
                        source: Some(anim_world_sound_source(world)),
                    });
                }
                // `0x006FA0C6 MOV ECX,0x843138 ("EVA_UnitPromoted") ; OR
                // EDX,-1 ; CALL VoxClass::PlayEVA`: the voice slot, with
                // the entry's own type (STANDARD LOW on stock data).
                #[cfg(not(feature = "ares"))]
                output.push(GameSoundEvent::Eva {
                    event: "EVA_UnitPromoted".to_string(),
                    type_override: None,
                });
                #[cfg(not(feature = "ares"))]
                collect_eva_object(&mut hud_text, "VERA:EvaUnitPromoted", type_id);
                // Ares UpdateVeterancy10046D4B..10046D54 queues the type's
                // EVA.*Promoted index instead; -1 queues nothing.
                #[cfg(feature = "ares")]
                if let Some(eva) = eva {
                    output.push(GameSoundEvent::Eva {
                        event: sim.interner().resolve(eva).to_string(),
                        type_override: None,
                    });
                    collect_eva_object(&mut hud_text, "VERA:EvaUnitPromoted", type_id);
                }
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
                // `HouseClass::IsHumanPlayer @ 0x0050B6F0` at every producer
                // (capture `0x00469267` / `0x004692FC`, bomb `0x00438FE6`,
                // airstrike abort `0x0041D5A5`, crate `0x004822A2`, repair
                // click `0x004470CF`, sale `0x0044A7AF`): the sound plays only
                // when a human player owns one of its houses.
                if let Some(houses) = audible_to
                    && !houses.iter().any(|&house| sim.house_is_human_player(house))
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
            SimSoundEvent::VocAtCoord {
                sound_id,
                coordinate,
                flags,
            } => {
                let coordinate = [coordinate.x, coordinate.y, coordinate.z];
                GameSoundEvent::VocAtCoord {
                    sound_id,
                    coordinate,
                    flags,
                    source: SoundSource::from_coordinate(coordinate),
                }
            }
            SimSoundEvent::VocStopAtCoord { coordinate, flags } => GameSoundEvent::VocStopAtCoord {
                coordinate: [coordinate.x, coordinate.y, coordinate.z],
                flags,
            },
            SimSoundEvent::BuildingComplete { owner, type_id } => {
                // Only play EVA for the local player's production.
                if !owner_is_local(sim.interner(), owner, local_owner_name) {
                    continue;
                }
                collect_eva_object(&mut hud_text, "VERA:EvaConstructionComplete", type_id);
                // `StripClass::AI 0x006A8E2F`: `PlayEVA` with type -1.
                GameSoundEvent::Eva {
                    event: "EVA_ConstructionComplete".to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::SuperWeaponActivation { sw_type, world } => {
                let Some(sw) = rules.super_weapon(sim.interner().resolve(sw_type)) else {
                    continue;
                };
                // Launch6CD176 uses PlayAt7509E0 before effect receivers.
                // No activation EVA belongs to the ForceShield case.
                if sw.kind == crate::rules::superweapon_type::SuperWeaponKind::ForceShield {
                    let cue =
                        superweapon_launch_cue(sw.kind, &rules.general, sw.start_sound.as_deref());
                    if let Some(sound_id) = cue.sound_id {
                        output.push(GameSoundEvent::VocAt {
                            sound_id,
                            source: Some(anim_world_sound_source(world)),
                        });
                    }
                }
                // Ares SW_Protect::Activate10074750: StartSound at the cell.
                #[cfg(feature = "ares")]
                if sw.kind == crate::rules::superweapon_type::SuperWeaponKind::Protect
                    && let Some(sound_id) = sw.start_sound.clone()
                {
                    output.push(GameSoundEvent::VocAt {
                        sound_id,
                        source: Some(anim_world_sound_source(world)),
                    });
                }
                continue;
            }
            SimSoundEvent::SuperWeaponLaunched {
                owner,
                sw_type,
                world,
            } => {
                // `SuperClass::Launch @ 0x006CC390` switches on the
                // launched type's `Type=` index and plays that case's
                // cue and/or EVA line. `superweapon_launch_cue` is that
                // table; both halves come off the one type object, as
                // native's `param_1[10]` does.
                let type_name = sim.interner().resolve(sw_type);
                let Some(sw) = rules.super_weapon(type_name) else {
                    continue;
                };
                // ForceShield's cue was dispatched at its early activation
                // boundary; this later fact still reaches the shared Ready
                // cleanup below without playing the cue twice.
                // A handler's launch (hook 6CC390) skips the stock case: its
                // EVA and cleanup are the common Launch's own facts, its
                // sounds the handler's.
                #[cfg(feature = "ares")]
                let ares_handler = sw.ares_eva().is_some();
                #[cfg(not(feature = "ares"))]
                let ares_handler = false;
                let mut cue = if ares_handler
                    || sw.kind == crate::rules::superweapon_type::SuperWeaponKind::ForceShield
                {
                    SuperWeaponLaunchCue::default()
                } else {
                    superweapon_launch_cue(sw.kind, &rules.general, sw.start_sound.as_deref())
                };
                // Cases 0, 7, 9 and 11 play their cue through
                // `VocClass::PlayAtCoord @ 0x00750E20` with flags 0
                // (`0x006CDCAE`/`0x006CDDE9`, `0x006CCE28`, `0x006CD8D3`,
                // `0x006CD7BF`): a retained request the sound pump re-pans
                // and stops out of earshot, not a one-shot. ForceShield's
                // `PlayAt` cue is not dispatched here.
                // It follows the case's EVA line (the silo arm of case 0
                // has them the other way round, `0x006CDDE9`/`0x006CDE01`).
                let retained_cue =
                    cue.positional
                        .then(|| cue.sound_id.take())
                        .flatten()
                        .map(|sound_id| {
                            let coordinate = [world.x, world.y, world.z];
                            GameSoundEvent::VocAtCoord {
                                sound_id,
                                coordinate,
                                flags: 0,
                                source: SoundSource::from_coordinate(coordinate),
                            }
                        });
                // The EVA column is picked for the *listening* client's
                // side, not the launcher's: `VoxClass::PlayEVA` takes
                // only the event name; the consumer resolves the side.
                let eva_event = (!ares_handler)
                    .then(|| {
                        eva_producers::resolved_superweapon_eva(
                            sw,
                            eva_producers::SuperWeaponEvaStage::Activated,
                            cue.eva_event,
                        )
                    })
                    .flatten()
                    .filter(|_| local_owner_name.is_some() && !sim.local_player_defeated())
                    .map(str::to_string);
                // What is left of the cue is its EVA line, with no position.
                let activated = eva_event.map(|line| GameSoundEvent::SuperWeaponActivated {
                    sound_id: String::new(),
                    source: None,
                    eva_event: Some(line),
                });
                // The player's tail ([`launch_drops_ready_line`]): after
                // the case's line, the queued Ready line is dropped.
                if let Some(ready) = launch_drops_ready_line(sw.kind).filter(|_| !ares_handler)
                    && owner_is_local(sim.interner(), owner, local_owner_name)
                {
                    for event in activated.into_iter().chain(retained_cue) {
                        output.push(event);
                    }
                    output.push(GameSoundEvent::EvaRemove {
                        event: ready.to_string(),
                    });
                    continue;
                }
                if let Some(activated) = activated {
                    output.push(activated);
                }
                let Some(retained_cue) = retained_cue else {
                    continue;
                };
                retained_cue
            }
            SimSoundEvent::RadarEvent { radar } => {
                // A type-13 event from `SuperClass::Launch` case 1
                // (`0x006CCF2F`), case 4 (`0x006CC4BE`, `0x006CC4D2`: the
                // source cell, then the target's), case 7 (`0x006CCDD7`) and
                // case 9 (`0x006CD8E0`), a storm's start (`0x00539F89`) or a
                // `NUKE` warhead's impact (`0x00467EA7`), or a type-12 one
                // from a `RevealToAll=` building's opening (`0x004468A8`),
                // with no local-player test; it plays nothing.
                let _ = admit_radar(radar);
                continue;
            }
            SimSoundEvent::LightningStormBegan => {
                // The undeferred half of `LightningStorm::Start`, which a
                // launch's countdown defers: `[AudioVisual] StormSound=`
                // (`[Rules+0x730]`) through `VocClass::PlayAtPos @
                // 0x00750920` (`0x0053A044`) with pan `0x2000` and volume
                // 1.0, centred rather than at the storm cell. Its line
                // follows through `hud_messages`; the EVA line
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
                // `hud_messages`).
                if local_owner_name.is_none() {
                    continue;
                }
                GameSoundEvent::Eva {
                    event: "EVA_LightningStormCreated".to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::UnitComplete {
                owner,
                radar,
                type_id,
            } => {
                // `HouseClass::Place_Production` inlines IsHumanPlayer
                // (`0x004FB5C6..0x004FB5EB`).
                if !sim.house_is_human_player(owner) {
                    continue;
                }
                // `HouseClass::Place_Production 0x004FB631`: the type-6 radar
                // accept gates the line; `0x004FB644` plays it with type -1.
                if !admit_radar(radar) {
                    continue;
                }
                collect_eva_object(&mut hud_text, "VERA:EvaUnitReady", type_id);
                GameSoundEvent::Eva {
                    event: "EVA_UnitReady".to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::MatchOutcome { owner, kind } => {
                // `Flag_To_Win` / `Flag_To_Lose` inline IsHumanPlayer
                // (`0x004FCB18..0x004FCB33`, `0x004FCD57`).
                if !sim.house_is_human_player(owner) {
                    continue;
                }
                GameSoundEvent::Eva {
                    event: outcome_eva_event(kind, !sim.session().game_mode_nonzero()).to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::WallSold { receiver } => {
                let receiver_name = sim.interner().resolve(receiver);
                let Some(event) =
                    wall_sell_sound_for_local(receiver_name, local_owner_name, Some(rules))
                else {
                    continue;
                };
                event
            }
            SimSoundEvent::CannotDeployHere { owner } => {
                if !owner_is_local(sim.interner(), owner, local_owner_name) {
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
                // ScoldSound through `0x00750920` for a human player's house
                // (`CALL 0x0050B6F0`).
                if !sim.house_is_human_player(owner) {
                    continue;
                }
                let Some(sound_id) = rules.general.scold_sound.clone() else {
                    continue;
                };
                GameSoundEvent::UiSound { sound_id }
            }
            SimSoundEvent::BuildingPlaced {
                owner,
                building_type,
            } => {
                // `0x004FB2CC..0x004FB314`: BuildingSlam through `0x00750920`
                // (pan `0x2000`, volume `1.0f`) for the player's house.
                if !owner_is_local(sim.interner(), owner, local_owner_name) {
                    continue;
                }
                let sound_id = rules.general.building_slam.as_deref();
                // Ares HouseClass_UnitFromFactory_BuildingSlam (10010290, hook
                // 0x4FB2FD): the placed type's SlamSound, else BuildingSlam.
                #[cfg(feature = "ares")]
                let sound_id = sim
                    .interner()
                    .object_type_in_category(
                        building_type,
                        crate::rules::object_type::ObjectCategory::Building,
                        rules,
                    )
                    .and_then(|object| object.slam_sound())
                    .unwrap_or(sound_id);
                #[cfg(not(feature = "ares"))]
                let _ = building_type;
                let Some(sound_id) = sound_id.map(str::to_owned) else {
                    continue;
                };
                GameSoundEvent::UiSound { sound_id }
            }
            SimSoundEvent::StructureGarrisoned { owner, type_id } => {
                // `0x005229AE CALL 0x0050B6F0`: only for a human player.
                if !sim.house_is_human_player(owner) {
                    continue;
                }
                collect_eva_object(&mut hud_text, "VERA:EvaStructureGarrisoned", type_id);
                // `BuildingClass::AddGarrisonOccupant 0x005229C1`: type -1
                // (stock entry QUEUE NORMAL).
                GameSoundEvent::Eva {
                    event: "EVA_StructureGarrisoned".to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::StructureAbandoned { radar, type_id, .. } => {
                //458200's producer already ran House50B6F0. Native gates
                //EVA on successful radar admission, before refreshing art.
                if !admit_radar(radar) {
                    continue;
                }
                collect_eva_object(&mut hud_text, "VERA:EvaStructureAbandoned", type_id);
                GameSoundEvent::Eva {
                    event: "EVA_StructureAbandoned".to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::BuildingGarrisonedSfx { owner, world } => {
                // Positional SFX behind the same IsHumanPlayer test
                // (`0x005229AE`) as the EVA line.
                if !sim.house_is_human_player(owner) {
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
                    source: Some(anim_world_sound_source(world)),
                }
            }
            SimSoundEvent::ChuteSound { world } => {
                let sound_id = match Some(rules).and_then(|r| r.general.chute_sound.as_deref()) {
                    Some(s) if !s.is_empty() => s.to_string(),
                    _ => continue,
                };
                GameSoundEvent::ChuteSound {
                    sound_id,
                    source: Some(anim_world_sound_source(world)),
                }
            }
            SimSoundEvent::BunkerWallsUp { world } => {
                // Walls-up cue on install; skip when the rules key is empty.
                let sound_id =
                    match Some(rules).and_then(|r| r.general.bunker_walls_up_sound.as_deref()) {
                        Some(s) if !s.is_empty() => s.to_string(),
                        _ => continue,
                    };
                GameSoundEvent::BunkerWalls {
                    sound_id,
                    source: Some(anim_world_sound_source(world)),
                }
            }
            SimSoundEvent::BunkerWallsDown { world } => {
                // Walls-down cue on normal exit / clear teardown.
                let sound_id =
                    match Some(rules).and_then(|r| r.general.bunker_walls_down_sound.as_deref()) {
                        Some(s) if !s.is_empty() => s.to_string(),
                        _ => continue,
                    };
                GameSoundEvent::BunkerWalls {
                    sound_id,
                    source: Some(anim_world_sound_source(world)),
                }
            }
            SimSoundEvent::BridgeRepaired { world, radar } => {
                // Spatial SFX gated on rules.bridge_rules.repair_sound
                // being set (the original game gates on
                // `RulesClass+0x248 != -1`).
                let sound_id = Some(rules)
                    .and_then(|r| r.bridge_rules.repair_sound.clone())
                    .unwrap_or_default();
                let source = if sound_id.is_empty() {
                    None
                } else {
                    Some(anim_world_sound_source(world))
                };
                // Infantry519BC9 calls EVA only after House50B6F0 (the sim
                // published `radar`) and radar insertion `0x00519BB6`
                // admitted it. A second owner filter here would incorrectly
                // suppress mode-0 PlayerControl output.
                let eva_event = radar
                    .is_some_and(&mut *admit_radar)
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
                let is_local = owner_is_local(sim.interner(), owner, local_owner_name);
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
                if !owner_is_local(sim.interner(), owner, local_owner_name) || !admit_radar(radar) {
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
            SimSoundEvent::UnitLost {
                owner,
                radar,
                type_id,
                #[cfg(feature = "ares")]
                eva,
                #[cfg(feature = "ares")]
                radar_event,
            } => {
                // `TechnoClass::Death_Announcement 0x004D98CA..0x004D9911`:
                // `PlayEVA("EVA_UnitLost", -1)` when IsHumanPlayer accepts
                // the owner, the sim's Spawned gate passed and this radar
                // type-7 request is admitted.
                #[cfg(not(feature = "ares"))]
                let event = {
                    if !sim.house_is_human_player(owner) || !admit_radar(radar) {
                        continue;
                    }
                    "EVA_UnitLost".to_string()
                };
                // Ares FootClass_Destroyed (10049080) creates the radar event
                // and plays the type's EVA.Lost whatever it answers;
                // BuildingClass_Destroyed_UnitLost (10013F10) creates none.
                #[cfg(feature = "ares")]
                let event = {
                    if !sim.house_is_human_player(owner) {
                        continue;
                    }
                    if radar_event {
                        let _ = admit_radar(radar);
                    }
                    sim.interner().resolve(eva).to_string()
                };
                collect_eva_object(&mut hud_text, "VERA:EvaUnitLost", type_id);
                GameSoundEvent::Eva {
                    event,
                    type_override: None,
                }
            }
            #[cfg(not(feature = "ares"))]
            SimSoundEvent::SuperWeaponInsufficientFunds { .. } => continue,
            #[cfg(feature = "ares")]
            SimSoundEvent::AresSuperWeaponRefused { .. } => continue,
            #[cfg(feature = "ares")]
            SimSoundEvent::AresSuperWeaponActivated { .. }
            | SimSoundEvent::AresLightningStormApproaching { .. }
            | SimSoundEvent::AresSuperWeaponAborted { .. } => continue,
            // 100731FD..10073236: IsAlliedWith(PlayerPtr) on the owner.
            #[cfg(feature = "ares")]
            SimSoundEvent::AresAlliedRadarEvent { owner, radar } => {
                let local = local_owner_name.and_then(|name| {
                    sim.houses()
                        .order()
                        .iter()
                        .copied()
                        .find(|&house| sim.interner().resolve(house).eq_ignore_ascii_case(name))
                });
                if local.is_some_and(|local| {
                    sim.house_alliances()
                        .is_allied(sim.interner(), owner, local)
                }) {
                    let _ = admit_radar(radar);
                }
                continue;
            }
            // Common Launch10032FC3: VoxClass::PlayIndex of the data type's
            // EVA_Activated, on every client behind the launch-mute byte.
            #[cfg(feature = "ares")]
            SimSoundEvent::AresSuperWeaponActivatedEva { sw_type } => {
                let Some(event) = rules
                    .super_weapon(sim.interner().resolve(sw_type))
                    .and_then(|sw| sw.ares_eva())
                    .and_then(|eva| eva.activated())
                    .filter(|_| local_owner_name.is_some() && !sim.local_player_defeated())
                else {
                    continue;
                };
                GameSoundEvent::SuperWeaponActivated {
                    sound_id: String::new(),
                    source: None,
                    eva_event: Some(event.to_string()),
                }
            }
            // SuperClass_Update_DrainMoney100358A0 plays the cue on every
            // client while the House is ControlledByHuman (the sim's gate).
            #[cfg(feature = "ares")]
            SimSoundEvent::AresSuperWeaponDrainRefused { sw_type, .. } => {
                let Some(event) = sim
                    .interner()
                    .try_resolve(sw_type)
                    .and_then(|name| rules.super_weapon(name))
                    .and_then(|sw| sw.insufficient_funds_eva.as_deref())
                else {
                    continue;
                };
                GameSoundEvent::Eva {
                    event: event.to_owned(),
                    type_override: None,
                }
            }
            #[cfg(feature = "ares")]
            SimSoundEvent::SuperWeaponInsufficientFunds { owner, sw_type } => {
                if !owner_is_local(sim.interner(), owner, local_owner_name) {
                    continue;
                }
                let Some(event) = sim
                    .interner()
                    .try_resolve(sw_type)
                    .and_then(|name| rules.super_weapon(name))
                    .and_then(|sw| sw.insufficient_funds_eva.as_deref())
                else {
                    continue;
                };
                GameSoundEvent::Eva {
                    event: event.to_owned(),
                    type_override: None,
                }
            }
            SimSoundEvent::HouseEva { owner, event } => {
                // `HouseClass::Update 0x004F8BA0` / `0x004F8D14`, the Robot
                // Tank lines (`0x0050E0D0`, `0x0050E19B`, `0x00502776`) and
                // the others: `PlayEVA(name, -1)` for the player's house —
                // the entry's own `Type=` and `Priority=` route it (stock:
                // `EVA_InsufficientFunds` STANDARD NORMAL, `EVA_LowPower`
                // QUEUE IMPORTANT).
                if !owner_is_local(sim.interner(), owner, local_owner_name) {
                    continue;
                }
                GameSoundEvent::Eva {
                    event: event.to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::StructureSold { owner, type_id } => {
                // `BuildingClass::Mission_Selling 0x00449CC1 MOV AL,[EBP+0x41A]`
                // (`0x0044AB22` on the upgrade path): only the local
                // player's own building speaks; `EDX = -1`.
                if !owner_is_local(sim.interner(), owner, local_owner_name) {
                    continue;
                }
                collect_eva_object(&mut hud_text, "VERA:EvaStructureSold", type_id);
                GameSoundEvent::Eva {
                    event: eva_producers::EVA_STRUCTURE_SOLD.to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::SellClick { owner } => {
                // `BuildingClass::Sell_Back 0x0044719B CALL 0x0050B6F0`
                // (IsHumanPlayer), then `VocClass::PlayAtPos 0x00750920` of
                // `Rules+0x70C` (`0x004471B6`).
                if !sim.house_is_human_player(owner) {
                    continue;
                }
                let Some(sound_id) = rules.general.generic_click_sound.clone() else {
                    continue;
                };
                GameSoundEvent::UiSound { sound_id }
            }
            SimSoundEvent::Repairing { owner, type_id } => {
                // `BuildingClass::ToggleRepair 0x004470A4 CALL
                // 0x0050B6F0` / depot MissionRepair44C4F3 local+41A.
                if !owner_is_local(sim.interner(), owner, local_owner_name) {
                    continue;
                }
                collect_eva_object(&mut hud_text, "VERA:EvaRepairing", type_id);
                GameSoundEvent::Eva {
                    event: eva_producers::EVA_REPAIRING.to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::UnitRepaired {
                owner,
                owner_admitted,
                radar,
            } => {
                // Hospital44B93F already admitted House50B6F0. Depot44BD96
                // defers its local+41A test; both radar insertions gate EVA
                // (44B973/44BDC5). Do not discard an admitted campaign house.
                if (!owner_admitted && !owner_is_local(sim.interner(), owner, local_owner_name))
                    || !admit_radar(radar)
                {
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
                #[cfg(feature = "ares")]
                ares_tech,
            } => {
                // `BuildingClass::ChangeOwner 0x004483C6/0x004483D1
                // CALL 0x0050B6F0` on the old and the new owner; the
                // lines are `eva_producers::capture_eva_events`. Every
                // call passes `EDX = -1` (the `0x00448459 QueueVoice`
                // passes `-1, -1` too).
                let local_is_old = sim.house_is_human_player(old_owner);
                let local_is_new = sim.house_is_human_player(new_owner);
                // `0x00448477 CreateRadarEvent(10)` is reached only past the
                // old-or-new-owner `0x0050B6F0` tests (`0x004483C6`,
                // `0x004483D1`; flag read at `0x004483EF`).
                let radar_accepted =
                    (local_is_old || local_is_new) && radar.is_some_and(&mut *admit_radar);
                let capture_event = capture_eva_event.map(|id| sim.interner().resolve(id));
                // Ares 3.0 10013E40 (hook 0x0044840B) plays the type's
                // LostEvaEvent instead.
                #[cfg(feature = "ares")]
                let tech_lost = ares_tech
                    .lost_eva_event
                    .map(|id| sim.interner().resolve(id));
                #[cfg(not(feature = "ares"))]
                let tech_lost = Some(eva_producers::EVA_TECH_BUILDING_LOST);
                for event in eva_producers::capture_eva_events(
                    tech_building,
                    radar_accepted,
                    capture_event,
                    tech_lost,
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
            SimSoundEvent::BuildingInfiltrated {
                owner,
                infiltrator,
                effect,
                radar,
            } => {
                // `BuildingClass::Infiltrate 0x004571FE/0x0045720E CALL
                // 0x0050B6F0` on the owner, then the spy's house: only then
                // `0x0045722F CreateRadarEvent(9)`. The lines are
                // `eva_producers::infiltration_eva_events`; every `PlayEVA`
                // call passes `EDX = -1`.
                let local_is_owner = sim.house_is_human_player(owner);
                let local_is_infiltrator = sim.house_is_human_player(infiltrator);
                let radar_accepted = (local_is_owner || local_is_infiltrator) && admit_radar(radar);
                for event in eva_producers::infiltration_eva_events(
                    effect,
                    radar_accepted,
                    local_is_owner,
                    local_is_infiltrator,
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
                if !owner_is_local(sim.interner(), owner, local_owner_name) {
                    continue;
                }
                let type_name = sim.interner().resolve(sw_type);
                let Some(event) = rules.super_weapon(type_name).and_then(|sw| {
                    eva_producers::resolved_superweapon_eva(
                        sw,
                        eva_producers::SuperWeaponEvaStage::Ready,
                        eva_producers::super_weapon_ready_event(sw.kind),
                    )
                }) else {
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
            // The refusal's line follows through `hud_messages`;
            // its AddMessage plays the only sound.
            SimSoundEvent::LightningStormRefused { .. }
            | SimSoundEvent::PsychicDominatorRefused { .. } => continue,
            #[cfg(feature = "ares")]
            SimSoundEvent::AresSuperWeaponFinished { owner, sw_type, .. } => {
                if !owner_is_local(sim.interner(), owner, local_owner_name) {
                    continue;
                }
                let Some(event) = rules
                    .super_weapon(sim.interner().resolve(sw_type))
                    .and_then(|sw| sw.ares_eva())
                    .and_then(|eva| eva.ready())
                else {
                    continue;
                };
                GameSoundEvent::EvaRemove {
                    event: event.to_string(),
                }
            }
            SimSoundEvent::SuperWeaponDetected { owner, sw_type } => {
                // `BuildingClass::OnConstructionComplete
                // 0x004468AD..0x00446995`; the gates are the
                // listener's (`eva_producers::super_weapon_detected_allowed`)
                // and the line comes off the `[SuperWeaponTypes]`
                // list index (`0x00446948` jump table).
                if !super_weapon_detection_admitted(sim, rules, owner, sw_type, local_owner_name) {
                    continue;
                }
                let type_name = sim.interner().resolve(sw_type).to_string();
                let Some(event) = rules
                    .super_weapon_order
                    .iter()
                    .position(|section| section.eq_ignore_ascii_case(&type_name))
                    .and_then(|index| {
                        let sw = rules.super_weapon(&type_name)?;
                        eva_producers::resolved_superweapon_eva(
                            sw,
                            eva_producers::SuperWeaponEvaStage::Detected,
                            eva_producers::super_weapon_detected_event(sw.kind, index),
                        )
                    })
                else {
                    continue;
                };
                GameSoundEvent::Eva {
                    event: event.to_string(),
                    type_override: None,
                }
            }
            // VoxClass 0x00752700 from MakeAlly/MakeEnemy; the sim chose the
            // local client's line.
            SimSoundEvent::AllianceChanged { eva: Some(eva), .. } => GameSoundEvent::Eva {
                event: eva.event().to_string(),
                type_override: None,
            },
            SimSoundEvent::AllianceChanged { eva: None, .. } => continue,
            SimSoundEvent::PlayerDefeated { house } => {
                // `HouseClass::MPlayer_Defeated 0x004FC1B5 CMP EAX,ESI`
                // splits the local `EVA_YouHaveLost` (`0x004FC2EA`) from the
                // remote `EVA_PlayerDefeated` (`0x004FC3BC`). A Flag_To_Lose
                // in the same frame queues the local line again, and
                // QueueVoice drops that same-type duplicate (`0x00752566`).
                let event = if owner_is_local(sim.interner(), house, local_owner_name) {
                    "EVA_YouHaveLost"
                } else {
                    eva_producers::EVA_PLAYER_DEFEATED
                };
                GameSoundEvent::Eva {
                    event: event.to_string(),
                    type_override: None,
                }
            }
        };
        output.push(app_event);
    }
}

/// `BuildingClass::OnConstructionComplete 0x004468AD..0x00446935`'s
/// listener gates for one detection fact: the EVA line and, with Ares, the
/// `Message.Detected` line its hook (100349D0) prints at the same point.
fn super_weapon_detection_admitted(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    owner: crate::sim::intern::InternedId,
    sw_type: crate::sim::intern::InternedId,
    local_owner_name: Option<&str>,
) -> bool {
    let Some(local_name) = local_owner_name else {
        return false;
    };
    let owner_name = sim.interner().resolve(owner).to_string();
    let type_name = sim.interner().resolve(sw_type).to_string();
    // `0x004468FA..0x00446935`: `AuxBuilding=` absent, or the building's own
    // house owns one (`CountOwnedInstances` on `Owner+0x5550`). Optional Ares
    // lists use the same grant availability owner.
    #[cfg(not(feature = "ares"))]
    let aux_satisfied = rules.super_weapon(&type_name).is_none_or(|sw| {
        crate::sim::superweapon::super_weapon_available(sim, rules, owner, &sw.id)
    });
    #[cfg(feature = "ares")]
    let aux_satisfied =
        crate::sim::superweapon::super_weapon_available(sim, rules, owner, &type_name);
    eva_producers::super_weapon_detected_allowed(
        sim.house_alliances().as_map(),
        &owner_name,
        local_name,
        sim.local_player_defeated(),
        sim.session().game_mode_nonzero(),
        aux_satisfied,
    )
}

/// Ares 3.0 `BuildingClass_ChangeOwnership_Tech @ 10013E40` (hook
/// `0x0044840B`): a captured tech building prints `Message.Lost` when the
/// old owner is the player (`HouseClass::Player` outside a campaign), then
/// `Message.Capture` when the new owner is; an empty label prints nothing.
#[cfg(feature = "ares")]
pub(super) fn ares_tech_capture_messages(
    events: &[SimSoundEvent],
    sim: &crate::sim::world::Simulation,
    local_owner_name: Option<&str>,
) -> Vec<crate::app::input::messages::HudMessage> {
    use crate::app::input::messages::HudMessage;
    let local = local_owner_name.and_then(|name| sim.interner().get(name));
    let mut lines = Vec::new();
    for event in events {
        if let SimSoundEvent::BuildingCaptured {
            old_owner,
            new_owner,
            tech_building: true,
            ares_tech,
            ..
        } = *event
        {
            for (owner, label) in [
                (old_owner, ares_tech.message_lost),
                (new_owner, ares_tech.message_capture),
            ] {
                if let Some(label) = label
                    && Some(owner) == local
                {
                    lines.push(HudMessage::AresTechCaptureLabel(label));
                }
            }
        }
    }
    lines
}

/// The SWTypeExt::PrintMessage10033BD0 lines the frame's Super facts print,
/// beside their EVA lines: `Message.Detected` past the detection gates (hook
/// 4468F4, firer the owner), `Message.Ready` for the local owner's
/// readiness (hook 6CBDD7, firer the player), `Message.InsufficientFunds`
/// for its ClickFire refusal (10035130, firer the player), a money drain
/// refusal on a human-controlled House (100358A0, firer the player),
/// `Message.Launch` on every client after a new type's launch (Launch
/// 10032ED0, firer the owner) and `Message.Activate` when UnitDelivery's
/// deferment ends (10075DE0, firer the owner).
#[cfg(feature = "ares")]
pub(super) fn ares_superweapon_messages(
    events: &[SimSoundEvent],
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    local_owner_name: Option<&str>,
) -> Vec<crate::app::input::messages::HudMessage> {
    use crate::app::input::messages::HudMessage;
    use crate::rules::superweapon_type::AresSuperWeaponMessage as M;
    let interner = sim.interner();
    let local = local_owner_name.and_then(|name| interner.get(name));
    let line = |sw_type, message, firer| {
        Some(HudMessage::AresSuperWeaponMessage {
            sw_type,
            message,
            firer,
        })
    };
    events
        .iter()
        .filter_map(|event| match *event {
            SimSoundEvent::SuperWeaponDetected { owner, sw_type } => {
                super_weapon_detection_admitted(sim, rules, owner, sw_type, local_owner_name)
                    .then(|| line(sw_type, M::Detected, Some(owner)))
                    .flatten()
            }
            SimSoundEvent::SuperWeaponReady { owner, .. }
            | SimSoundEvent::SuperWeaponInsufficientFunds { owner, .. }
                if Some(owner) != local =>
            {
                None
            }
            SimSoundEvent::SuperWeaponReady { owner, sw_type } => {
                line(sw_type, M::Ready, Some(owner))
            }
            SimSoundEvent::SuperWeaponInsufficientFunds { owner, sw_type } => {
                line(sw_type, M::InsufficientFunds, Some(owner))
            }
            SimSoundEvent::AresSuperWeaponDrainRefused { sw_type, .. } => {
                line(sw_type, M::InsufficientFunds, local)
            }
            SimSoundEvent::AresSuperWeaponFinished {
                owner,
                sw_type,
                message: true,
            }
            | SimSoundEvent::AresLightningStormApproaching { owner, sw_type } => {
                line(sw_type, M::Launch, Some(owner))
            }
            SimSoundEvent::AresSuperWeaponActivated { owner, sw_type } => {
                line(sw_type, M::Activate, Some(owner))
            }
            // SW_PsychicDominator::AbortFire10070780: only for IsPlayer.
            SimSoundEvent::AresSuperWeaponAborted { owner, sw_type } if Some(owner) == local => {
                line(sw_type, M::Abort, Some(owner))
            }
            _ => None,
        })
        .collect()
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

/// The PlayerPtr test (`[0x00A83D4C]` compared inline, or the owned-by-player
/// byte `Techno+0x41A`): the owner is the local player, the scenario's current
/// house. Arms whose native sites call `HouseClass::IsHumanPlayer @
/// 0x0050B6F0` ask [`Simulation::house_is_human_player`] instead.
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
pub(crate) fn outcome_eva_event(
    kind: crate::sim::house_state::HouseOutcomeKind,
    campaign: bool,
) -> &'static str {
    //Campaign branches4FCB62/4FCD4B use distinct registered EVA names
    //at824C54/824CAC; multiplayer branches use824C2C/824C08.
    match (kind, campaign) {
        (crate::sim::house_state::HouseOutcomeKind::Victory, true) => "EVA_MissionAccomplished",
        (crate::sim::house_state::HouseOutcomeKind::Defeat, true) => "EVA_MissionFailed",
        (crate::sim::house_state::HouseOutcomeKind::Victory, false) => "EVA_YouAreVictorious",
        (crate::sim::house_state::HouseOutcomeKind::Defeat, false) => "EVA_YouHaveLost",
    }
}

pub(crate) fn anim_world_sound_source(
    world: crate::sim::anim_class::AnimWorldCoord,
) -> SoundSource {
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
/// (`HouseClass::MPlayer_Defeated @ 0x004FC205` sets it to 1); the dispatch
/// reads it as [`crate::sim::world::Simulation::local_player_defeated`].
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
        // New handlers resolve their retained Ares EVA above this stock table;
        // their common sound uses the separate ordered presentation fact.
        K::ChronoSphere
        | K::ParaDrop
        | K::AmerParaDrop
        | K::SpyPlane
        | K::NoEffect
        | K::GenericWarhead
        | K::UnitDelivery
        | K::HunterSeeker
        | K::Protect
        | K::Battery
        | K::SonarPulse
        | K::EMPulse
        | K::Firestorm
        | K::DropPod => SuperWeaponLaunchCue::default(),
    }
}

/// Ordered HUD facts from the existing House/Super producers. Every client posts
/// `TXT_LIGHTNING_STORM` when a storm starts (`LightningStorm::Start @
/// 0x0053A067`) and `TXT_LIGHTNING_STORM_APPROACHING` at the countdown's
/// warnings (`LightningStorm::Process @ 0x0053AB31`). Only the client whose
/// player owns a refused Super posts ClickFire's line for it:
/// `Msg:LightningStormActive` (`LightningStorm::PrintMessage @ 0x0053AE00`)
/// or `Msg:DominatorActive` (`PsyDom::PrintMessage @ 0x0053B410`).
/// House4F8D5C posts low power only for its local owner, after EVA4F8D14.
pub(super) fn hud_messages(
    events: &[SimSoundEvent],
    interner: &crate::sim::intern::StringInterner,
    local_owner_name: Option<&str>,
) -> Vec<crate::app::input::messages::HudMessage> {
    use crate::app::input::messages::HudMessage;
    events
        .iter()
        .filter_map(|event| match *event {
            SimSoundEvent::LightningStormBegan => {
                Some(HudMessage::SuperWeapon("TXT_LIGHTNING_STORM"))
            }
            SimSoundEvent::LightningStormApproaching => {
                Some(HudMessage::SuperWeapon("TXT_LIGHTNING_STORM_APPROACHING"))
            }
            SimSoundEvent::LightningStormRefused { owner } => {
                owner_is_local(interner, owner, local_owner_name)
                    .then_some(HudMessage::SuperWeapon("Msg:LightningStormActive"))
            }
            SimSoundEvent::PsychicDominatorRefused { owner } => {
                owner_is_local(interner, owner, local_owner_name)
                    .then_some(HudMessage::SuperWeapon("Msg:DominatorActive"))
            }
            #[cfg(feature = "ares")]
            SimSoundEvent::AresSuperWeaponRefused { owner, sw_type } => {
                // Ares10038ECE compares the event House with PlayerPtr.
                owner_is_local(interner, owner, local_owner_name)
                    .then_some(HudMessage::AresSuperWeaponRefused(sw_type))
            }
            // House4F8D14/4F8D5C emits voice and text at the same admitted
            // guard edge. Consume the fact even if audio was delivered inline.
            SimSoundEvent::HouseEva {
                owner,
                event: crate::sim::house_eva::EVA_LOW_POWER,
            } => owner_is_local(interner, owner, local_owner_name).then_some(HudMessage::LowPower),
            // Both MPlayer_Defeated branches post the line; the sim already
            // left out the observer House.
            SimSoundEvent::PlayerDefeated { house } => Some(HudMessage::PlayerDefeated(house)),
            // Every client prints the line; the sim decided whether it is due.
            SimSoundEvent::AllianceChanged {
                owner,
                other,
                formed,
                line: true,
                ..
            } => Some(HudMessage::Alliance {
                owner,
                other,
                formed,
            }),
            _ => None,
        })
        .collect()
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
    use crate::app::input::messages::HudMessage;
    use crate::util::fixed_math::SimFixed;

    fn dispatch_rules() -> RuleSet {
        RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[General]\nFixtureOnly=1\n[InfantryTypes]\n0=E1\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [E1]\nVoiceFeedback=Feedback\n\
             [AudioVisual]\nBaseUnderAttackSound=Siren\n",
        ))
        .unwrap()
    }

    #[cfg(feature = "ares")]
    #[test]
    fn hunter_seeker_eva_runs_after_payload_and_cleans_ready_after_sound() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
        use crate::rules::sound_ini::{EvaRegistry, SoundRegistry};
        use crate::sim::command::Command;
        use crate::sim::superweapon::SuperWeaponInstance;
        use std::sync::Arc;
        let root = crate::sim::arena_fixture::clear_ground_ini(
            "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=HUNTER\n\
             [HUNTER]\nStrength=1\nSpeed=5\nSpeedType=Track\nMovementZone=Normal\n\
             Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             [BuildingTypes]\n0=LAUNCHER\n\
             [LAUNCHER]\nStrength=500\n[SuperWeaponTypes]\n0=Seek\n[Seek]\n\
             Type=HunterSeeker\nIsPowered=no\nHunterSeeker.Type=HUNTER\n\
             HunterSeeker.Buildings=LAUNCHER\nSW.Sound=Launch\n",
        );
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            root, None, IniFile::from_str("[LAUNCHER]\nFoundation=1x1\n"),
            Arc::new(SoundRegistry::from_ini(&IniFile::from_str("[SoundList]\n0=Launch\n"))),
            Arc::new(EvaRegistry::from_ini(&IniFile::from_str(
                "[DialogList]\n0=EVA_HunterSeekerReady\n1=EVA_HunterSeekerDetected\n2=EVA_HunterSeekerLaunched\n",
            ))),
        ).unwrap();
        let (rules, ..) = owner
            .load_scenario(
                NativeScenarioRulesPrefix::NonCampaign(None),
                &IniFile::empty(),
            )
            .unwrap()
            .into_parts();
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        let local = sim.intern("Americans");
        let enemy = sim.intern("Russians");
        let typ = sim.intern("Seek");
        let hear = |events: Vec<SimSoundEvent>, sim: &Simulation| {
            let mut output = SoundEventQueue::new();
            dispatch_sim_sound_events(
                events,
                sim,
                &rules,
                Some("Americans"),
                &mut |_| true,
                &mut output,
                None,
            );
            output.drain()
        };
        let heard = hear(
            vec![
                SimSoundEvent::SuperWeaponReady {
                    owner: local,
                    sw_type: typ,
                },
                SimSoundEvent::SuperWeaponReady {
                    owner: enemy,
                    sw_type: typ,
                },
                SimSoundEvent::SuperWeaponDetected {
                    owner: local,
                    sw_type: typ,
                },
                SimSoundEvent::SuperWeaponDetected {
                    owner: enemy,
                    sw_type: typ,
                },
            ],
            &sim,
        );
        let names: Vec<_> = heard
            .iter()
            .filter_map(|event| match event {
                GameSoundEvent::Eva { event, .. } => Some(event.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(names, ["EVA_HunterSeekerReady", "EVA_HunterSeekerDetected"]);
        sim.spawn_object_at_height("LAUNCHER", "Americans", 10, 10, 0, 0, &rules)
            .unwrap();
        let mut sw = SuperWeaponInstance::new(typ, local, 0);
        sw.set_is_active_for_test(true);
        sw.set_is_ready_for_test(true);
        sim.super_weapons_mut_for_test()
            .entry(local)
            .or_default()
            .insert(typ, sw);
        sim.take_sound_events_for_test();
        assert!(sim.apply_command(
            "Americans",
            &Command::LaunchSuperWeapon {
                sw_type_id: typ,
                target_rx: 0,
                target_ry: 0,
            },
            Some(&rules)
        ));
        let heard = hear(sim.take_sound_events_for_test(), &sim);
        let order: Vec<_> = heard
            .iter()
            .filter_map(|event| match event {
                GameSoundEvent::SuperWeaponActivated {
                    eva_event: Some(event),
                    ..
                } => Some(event.as_str()),
                GameSoundEvent::VocAtCoord { sound_id, .. } => Some(sound_id.as_str()),
                GameSoundEvent::EvaRemove { event } => Some(event.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            order,
            [
                "EVA_HunterSeekerLaunched",
                "Launch",
                "EVA_HunterSeekerReady"
            ]
        );
        let heard = hear(
            vec![SimSoundEvent::AresSuperWeaponFinished {
                owner: enemy,
                sw_type: typ,
                message: true,
            }],
            &sim,
        );
        assert!(
            heard.is_empty(),
            "remote launches must not erase the listener's Ready queue"
        );
    }

    #[cfg(feature = "ares")]
    #[test]
    fn superweapon_money_refusal_eva_is_owner_local_configurable_and_silenceable() {
        for (setting, expected) in [
            ("", Some("EVA_InsufficientFunds")),
            ("EVA.InsufficientFunds=CustomFunds\n", Some("CustomFunds")),
            ("EVA.InsufficientFunds=none\n", None),
        ] {
            let ini = crate::rules::ini_parser::IniFile::from_str(&format!(
                "[SuperWeaponTypes]\n0=Paid\n[Paid]\nType=PsychicReveal\nMoney.Amount=-500\n{setting}"
            ));
            let processed = crate::rules::native_processing::RulesLayerStack::new(ini)
                .process_with_fixed_art(&crate::rules::ini_parser::IniFile::empty())
                .unwrap();
            let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
            let eva = crate::rules::sound_ini::EvaRegistry::from_ini(
                &crate::rules::ini_parser::IniFile::from_str(
                    "[DialogList]\n0=EVA_InsufficientFunds\n1=CustomFunds\n",
                ),
            );
            rules.bind_type_eva_references(processed.ini(), &eva);
            let mut sim = Simulation::new();
            let local = sim.intern("Americans");
            let other = sim.intern("Russians");
            let sw_type = sim.intern("Paid");
            let mut output = SoundEventQueue::new();
            dispatch_sim_sound_events(
                [local, other]
                    .map(|owner| SimSoundEvent::SuperWeaponInsufficientFunds { owner, sw_type }),
                &sim,
                &rules,
                Some("Americans"),
                &mut |_| panic!("money refusal must not create a radar event"),
                &mut output,
                None,
            );
            let events = output.drain();
            let cues: Vec<_> = events
                .iter()
                .map(|event| match event {
                    GameSoundEvent::Eva {
                        event,
                        type_override,
                    } => {
                        assert!(type_override.is_none());
                        event.as_str()
                    }
                    other => panic!("unexpected event {other:?}"),
                })
                .collect();
            assert_eq!(cues, expected.into_iter().collect::<Vec<_>>());
        }
    }

    /// A retail Slave Miner's self-click deploy speaks `VoiceDeploy=`
    /// through ClickedMission's Unload arm; muted voices speak nothing.
    #[test]
    fn a_self_click_deploy_speaks_voice_deploy() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = retail.rules;
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x6FFD3A);
        let miner = sim
            .spawn_object("SMIN", "Americans", 10, 10, 0, &rules)
            .expect("SMIN spawns");
        assert!(matches!(
            deploy_order_voice_event(&mut sim, &rules, miner, true),
            Some(GameSoundEvent::UnitVoiceRequested { speaker_id, ref sound_id })
                if speaker_id == miner && sound_id == "SlaveMinerDeployVoice"
        ));
        assert!(deploy_order_voice_event(&mut sim, &rules, miner, false).is_none());
    }

    #[test]
    fn trigger_eva_dispatch_preserves_original_index_history_without_listener_gates() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/trigger_action_values.json",
        ))
        .unwrap();
        let history = rows
            .iter()
            .find(|row| row["case_kind"] == "eva_history")
            .unwrap();
        let indices: Vec<i32> = history["history"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|step| step["calls"].as_array().unwrap())
            .filter(|call| call["call"] == "QueueVoice")
            .map(|call| call["index"].as_i64().unwrap() as i32)
            .collect();
        assert_eq!(indices.len(), 7);
        let mut output = SoundEventQueue::new();
        dispatch_sim_sound_events(
            indices
                .iter()
                .map(|&index| SimSoundEvent::TriggerEva { index }),
            &Simulation::new(),
            &dispatch_rules(),
            None,
            &mut |_| panic!("original TAction21 does not create or gate on a radar event"),
            &mut output,
            None,
        );
        let observed: Vec<_> = output
            .drain()
            .into_iter()
            .map(|event| match event {
                GameSoundEvent::TriggerEva { index } => index,
                other => panic!("unexpected event {other:?}"),
            })
            .collect();
        assert_eq!(observed, indices);
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
            sim.session_mut_for_test().set_game_mode(
                crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(
                    input["game_mode"].as_u64().unwrap() != 0,
                ),
            );
            let houses = input["houses"].as_array().unwrap();
            let owner_name = houses[input["current_owner"].as_u64().unwrap() as usize]["name"]
                .as_str()
                .unwrap();
            let local_name = houses[input["local_house"].as_u64().unwrap() as usize]["name"]
                .as_str()
                .unwrap();
            let owner = sim.intern(owner_name);
            let type_id = sim.intern("GAPILL");
            let sound_name = format!("NativeSound{}", native_sound["sound"].as_i64().unwrap());
            let events = [
                SimSoundEvent::VocCentered {
                    sound_id: sound_name.clone(),
                },
                SimSoundEvent::StructureAbandoned {
                    owner,
                    type_id,
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
                    None,
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
        let local = sim.intern("Local");
        let remote = sim.intern("Remote");
        // A multiplayer client, where IsHumanPlayer is the local house.
        sim.session_mut_for_test()
            .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(local));
        let request = RadarEventRequest::new(RadarEventType::UnitReady, 15, 15);
        let event = |owner| SimSoundEvent::UnitComplete {
            owner,
            radar: request,
            type_id: local,
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
            None,
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
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
            "Hills.mmx",
            &crate::rules::ini_parser::IniFile::from_str("[AudioVisual]\nBuildingSlam=Unknown\n"),
        ) else {
            return;
        };
        let rules = retail.rules;
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        let remote = sim.intern("Remote");
        let building_type = sim.intern("GAPOWR");
        let mut output = SoundEventQueue::new();
        dispatch_sim_sound_events(
            [
                SimSoundEvent::BuildingPlaced {
                    owner: remote,
                    building_type,
                },
                SimSoundEvent::BuildingPlaced {
                    owner: local,
                    building_type,
                },
            ],
            &sim,
            &rules,
            Some("LOCAL"),
            &mut |_| true,
            &mut output,
            None,
        );
        let emitted = output.drain();
        assert_eq!(emitted.len(), 1);
        assert!(matches!(
            &emitted[0],
            GameSoundEvent::UiSound { sound_id } if sound_id == "PlaceBuilding"
        ));
    }

    /// Ares HouseClass_UnitFromFactory_BuildingSlam (10010290, hook
    /// 0x4FB2FD): the placed type's `SlamSound` replaces BuildingSlam, and
    /// `none` silences it.
    #[cfg(feature = "ares")]
    #[test]
    fn ares_slam_sound_replaces_the_building_slam() {
        use crate::rules::ini_parser::IniFile;
        let ini = IniFile::from_str(
            "[BuildingTypes]\n0=LOUD\n1=QUIET\n[LOUD]\nSlamSound=Slam\n[QUIET]\nSlamSound=none\n",
        );
        let mut rules = crate::rules::ruleset::RuleSet::from_ini(&ini).unwrap();
        let sounds = crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n0=Slam\n",
        ));
        rules.bind_type_sound_references(&ini, &sounds);
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        let [loud, quiet] = ["LOUD", "QUIET"].map(|name| sim.intern(name));
        let mut output = SoundEventQueue::new();
        dispatch_sim_sound_events(
            [loud, quiet].map(|building_type| SimSoundEvent::BuildingPlaced {
                owner: local,
                building_type,
            }),
            &sim,
            &rules,
            Some("LOCAL"),
            &mut |_| true,
            &mut output,
            None,
        );
        let emitted = output.drain();
        assert_eq!(emitted.len(), 1);
        assert!(matches!(
            &emitted[0],
            GameSoundEvent::UiSound { sound_id } if sound_id == "Slam"
        ));
    }

    /// PlanMgr638120 emits the add cue only after node admission; dispatch
    /// preserves that event and plays it solely for its local owning house.
    #[test]
    fn planning_add_sound_uses_retail_rule_and_local_admitted_event() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let mut rules = retail.rules;
        let expected = rules
            .general
            .add_planning_mode_command_sound
            .clone()
            .expect("retail planning add sound");
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        let remote = sim.intern("Remote");
        let events = [
            SimSoundEvent::PlanningNodeAdded { owner: remote },
            SimSoundEvent::PlanningNodeAdded { owner: local },
        ];
        let mut output = SoundEventQueue::new();
        dispatch_sim_sound_events(
            events.clone(),
            &sim,
            &rules,
            Some("LOCAL"),
            &mut |_| panic!("planning add is not a radar event"),
            &mut output,
            None,
        );
        assert!(matches!(output.drain().as_slice(),
            [GameSoundEvent::UiSound { sound_id }] if *sound_id == expected));
        dispatch_sim_sound_events(
            events.clone(),
            &sim,
            &rules,
            None,
            &mut |_| panic!("planning add is not a radar event"),
            &mut output,
            None,
        );
        assert!(output.drain().is_empty());
        rules.general.add_planning_mode_command_sound = None;
        dispatch_sim_sound_events(
            events,
            &sim,
            &rules,
            Some("Local"),
            &mut |_| panic!("planning add is not a radar event"),
            &mut output,
            None,
        );
        assert!(output.drain().is_empty());
    }

    #[test]
    fn dispatcher_preserves_batch_and_multi_cue_order_and_listener_gates() {
        let rules = dispatch_rules();
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        let remote = sim.intern("Remote");
        // A multiplayer client, where IsHumanPlayer is the local house.
        sim.session_mut_for_test()
            .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(local));
        let sound = sim.intern("Promotion");
        let promoted_type = sim.intern("E1");
        #[cfg(feature = "ares")]
        let eva = sim.intern("EVA_UnitPromoted");
        let promotion = |owner| SimSoundEvent::UnitPromoted {
            owner,
            sound_id: Some(sound),
            type_id: promoted_type,
            // The centre of cell (5, 7) at ground level.
            world: crate::sim::anim_class::AnimWorldCoord {
                x: 5 * 256 + 128,
                y: 7 * 256 + 128,
                z: 0,
            },
            #[cfg(feature = "ares")]
            eva: Some(eva),
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
        #[cfg(feature = "ares")]
        let lost_eva = sim.intern("EVA_UnitLost");
        let unit_lost = |owner, rx| SimSoundEvent::UnitLost {
            owner,
            radar: RadarEventRequest::new(RadarEventType::UnitLost, rx, 7),
            type_id: promoted_type,
            #[cfg(feature = "ares")]
            eva: lost_eva,
            #[cfg(feature = "ares")]
            radar_event: true,
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
            None,
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
        let mut expected = vec![
            "Earlier",
            "Promotion",
            "EVA_UnitPromoted",
            "EVA_OurBaseIsUnderAttack",
            "Siren",
            "EVA_OreMinerUnderAttack",
            "EVA_UnitLost",
        ];
        // Ares 10049080 speaks EVA.Lost even for the refused radar event.
        #[cfg(feature = "ares")]
        expected.push("EVA_UnitLost");
        assert_eq!(
            events
                .iter()
                .map(GameSoundEvent::sound_id)
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(events[1].source().unwrap().cell(), (5, 7));
        assert!(matches!(
            events[4],
            GameSoundEvent::BaseUnderAttackSfx { .. }
        ));
    }

    #[test]
    fn depot_and_hospital_eva_use_their_native_listener_and_radar_admission() {
        use crate::sim::house_state::HouseState;
        use crate::sim::radar::{RadarEventRequest, RadarEventType};
        let rules = dispatch_rules();
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        let remote = sim.intern("Remote");
        let controlled = sim.intern("Alliance");
        let mut house = HouseState::new(controlled, 0, None, false, 0, 10);
        house.set_player_control_for_test(true);
        sim.houses_mut_for_test().register(house);
        assert!(sim.house_is_human_player(controlled));
        let repaired = |owner, rx, owner_admitted| SimSoundEvent::UnitRepaired {
            owner,
            owner_admitted,
            radar: RadarEventRequest::new(RadarEventType::UnitRepaired, rx, 7),
        };
        let repair_type = sim.intern("GAPOWR");
        let mut admitted = Vec::new();
        let mut output = SoundEventQueue::new();
        dispatch_sim_sound_events(
            [
                repaired(remote, 5, false),
                repaired(controlled, 6, false),
                repaired(controlled, 8, true),
                SimSoundEvent::Repairing {
                    owner: local,
                    type_id: repair_type,
                },
                SimSoundEvent::HouseEva {
                    owner: local,
                    event: "EVA_InsufficientFunds",
                },
                repaired(local, 99, false),
                repaired(local, 5, false),
            ],
            &sim,
            &rules,
            Some("LOCAL"),
            &mut |request| {
                admitted.push((request.event_type, request.rx));
                request.rx != 99
            },
            &mut output,
            None,
        );
        assert_eq!(
            admitted,
            [
                (RadarEventType::UnitRepaired, 8),
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
            [
                "EVA_UnitRepaired",
                "EVA_Repairing",
                "EVA_InsufficientFunds",
                "EVA_UnitRepaired"
            ],
        );
    }

    /// `TechnoClass::Death_Announcement` asks IsHumanPlayer (`0x004D98CA`) of
    /// the lost object's owner. In a campaign that accepts a `PlayerControl=`
    /// house that is not the player, as ALL01's Alliance is when
    /// `Kill_Harriers` destroys its two ORCAs: the radar event and
    /// EVA_UnitLost follow. A multiplayer client announces only its own losses.
    #[test]
    fn a_campaign_player_control_house_announces_its_lost_unit() {
        use crate::sim::house_state::HouseState;
        use crate::sim::radar::{RadarEventRequest, RadarEventType};
        let rules = dispatch_rules();
        let mut sim = Simulation::new();
        let player = sim.intern("Americans");
        let ally = sim.intern("Alliance");
        let lost_type = sim.intern("E1");
        sim.houses_mut_for_test()
            .register(HouseState::new(player, 0, None, true, 0, 10));
        let mut house = HouseState::new(ally, 0, None, false, 0, 10);
        house.set_player_control_for_test(true);
        sim.houses_mut_for_test().register(house);
        let radar = RadarEventRequest::new(RadarEventType::UnitLost, 20, 30);
        #[cfg(feature = "ares")]
        let lost_eva = sim.intern("EVA_UnitLost");
        let lost = |sim: &Simulation| {
            let mut asked = 0;
            let mut output = SoundEventQueue::new();
            dispatch_sim_sound_events(
                [SimSoundEvent::UnitLost {
                    owner: ally,
                    radar,
                    type_id: lost_type,
                    #[cfg(feature = "ares")]
                    eva: lost_eva,
                    #[cfg(feature = "ares")]
                    radar_event: true,
                }],
                sim,
                &rules,
                Some("Americans"),
                &mut |_| {
                    asked += 1;
                    true
                },
                &mut output,
                None,
            );
            let spoken: Vec<String> = output
                .drain()
                .iter()
                .map(|event| event.sound_id().to_string())
                .collect();
            (asked, spoken)
        };
        assert_eq!(lost(&sim), (1, vec!["EVA_UnitLost".to_string()]));

        sim.session_mut_for_test()
            .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(player));
        assert_eq!(lost(&sim), (0, Vec::new()));
    }

    /// `tools/spy_infiltration_oracle.json`: the radar event and the EVA
    /// lines `BuildingClass::Infiltrate` (`0x004571E0`) asks for on the local
    /// player's machine, through the dispatcher and its radar admission.
    #[test]
    fn infiltration_announcements_match_the_original() {
        use crate::sim::spy_infiltration::spy_infiltration_tests::{HOUSES, replay, rows};
        for row in rows() {
            let name = row["name"].as_str().unwrap();
            let local = HOUSES
                .iter()
                .find(|(role, _)| row["local"] == *role)
                .unwrap()
                .1;
            let mut replayed = replay(&row);
            // The local player's machine: PlayerPtr is the local house.
            let local_id = replayed.sim.intern(local);
            replayed
                .sim
                .session_mut_for_test()
                .set_current_house_for_test(Some(local_id));
            let events = replayed.sim.take_sound_events_for_test();
            let mut asked = Vec::new();
            let mut output = SoundEventQueue::new();
            dispatch_sim_sound_events(
                events,
                &replayed.sim,
                &replayed.rules,
                Some(local),
                &mut |request| {
                    asked.push(serde_json::json!([
                        request.event_type as i32,
                        [request.rx, request.ry]
                    ]));
                    row["radar_accepted"].as_bool().unwrap()
                },
                &mut output,
                None,
            );
            let expected = row["expected"]["events"].as_array().unwrap();
            let of_kind = |kind: &str| -> Vec<serde_json::Value> {
                expected
                    .iter()
                    .filter(|event| event[0] == kind)
                    .map(|event| match kind {
                        "radar_event" => serde_json::json!([event[1], event[2]]),
                        _ => event[1].clone(),
                    })
                    .collect()
            };
            assert_eq!(asked, of_kind("radar_event"), "{name}: the radar event");
            let spoken: Vec<serde_json::Value> = output
                .drain()
                .iter()
                .map(|event| serde_json::json!(event.sound_id()))
                .collect();
            assert_eq!(spoken, of_kind("eva"), "{name}: the EVA lines");
        }
    }

    #[test]
    fn dispatcher_preserves_resolved_feedback_before_the_building_cue() {
        let rules = dispatch_rules();
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
                SimSoundEvent::VocAtCoord {
                    sound_id: "Damaged".into(),
                    coordinate: crate::sim::components::DriveCoord {
                        x: 3 * 256 + 61,
                        y: 4 * 256 + 174,
                        z: 104,
                    },
                    flags: 0,
                },
            ],
            &sim,
            &rules,
            Some("DifferentListener"),
            &mut |_| panic!("these cues have no radar gate"),
            &mut output,
            None,
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
        assert!(matches!(&events[1], GameSoundEvent::VocAtCoord { .. }));
        assert_eq!(events[1].source(), Some(source));
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

    /// Launch case 4's tail (`0x006CCCF0..0x006CCD2D`): `PlayEVA`
    /// (`EVA_ChronosphereActivated`) for every launcher, then for the
    /// launching player only `RemoveFromQueues(EVA_ChronosphereReady)`; its
    /// two radar events reach the client's radar and play nothing.
    // Installed Ares hands ChronoWarp and PsychicDominator to their handlers
    // (hook 6CC390), whose line and cleanup are the common Launch's own facts
    // (`ares_launch_eva_follows_the_data_type_and_skips_the_stock_tail`).
    #[cfg(not(feature = "ares"))]
    #[test]
    fn launch_drops_the_ready_line_for_its_player_only() {
        launch_drops_the_ready_line(
            "ChronoWarpSpecial",
            "EVA_ChronosphereActivated",
            "EVA_ChronosphereReady",
        );
        launch_drops_the_ready_line(
            "PsychicDominatorSpecial",
            "EVA_PsychicDominatorActivated",
            "EVA_PsychicDominatorReady",
        );
    }

    /// Common Ares Launch (10032ED0) for a redirected ChronoWarp: the stock
    /// case's line and Ready drop stay out of the launch fact; the
    /// activation line is the data type's (the ChronoSphere's), heard on
    /// every client, and its Ready leaves the launching player's queue.
    #[cfg(feature = "ares")]
    #[test]
    fn ares_launch_eva_follows_the_data_type_and_skips_the_stock_tail() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
        use crate::rules::sound_ini::{EvaRegistry, SoundRegistry};
        use std::sync::Arc;
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            IniFile::from_str(
                "[SuperWeaponTypes]\n0=Sphere\n1=Warp\n\
                 [Sphere]\nType=ChronoSphere\n[Warp]\nType=ChronoWarp\n",
            ),
            None,
            IniFile::empty(),
            Arc::new(SoundRegistry::from_ini(&IniFile::empty())),
            Arc::new(EvaRegistry::from_ini(&IniFile::from_str(
                "[DialogList]\n0=EVA_ChronosphereReady\n1=EVA_ChronosphereActivated\n",
            ))),
        )
        .unwrap();
        let (rules, ..) = owner
            .load_scenario(
                NativeScenarioRulesPrefix::NonCampaign(None),
                &IniFile::empty(),
            )
            .unwrap()
            .into_parts();
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        let remote = sim.intern("Remote");
        let (sphere, warp) = (sim.intern("Sphere"), sim.intern("Warp"));
        let mut output = SoundEventQueue::new();
        dispatch_sim_sound_events(
            [
                SimSoundEvent::SuperWeaponLaunched {
                    owner: local,
                    sw_type: warp,
                    world: crate::sim::components::DriveCoord::cell(40, 40, 0).into(),
                },
                SimSoundEvent::AresSuperWeaponActivatedEva { sw_type: sphere },
                SimSoundEvent::AresSuperWeaponFinished {
                    owner: remote,
                    sw_type: sphere,
                    message: true,
                },
                SimSoundEvent::AresSuperWeaponFinished {
                    owner: local,
                    sw_type: sphere,
                    message: true,
                },
            ],
            &sim,
            &rules,
            Some("LOCAL"),
            &mut |_| true,
            &mut output,
            None,
        );
        let emitted = output.drain();
        assert!(
            matches!(
                emitted.as_slice(),
                [
                    GameSoundEvent::SuperWeaponActivated { eva_event: Some(line), .. },
                    GameSoundEvent::EvaRemove { event },
                ] if line == "EVA_ChronosphereActivated" && event == "EVA_ChronosphereReady"
            ),
            "{emitted:?}"
        );
    }

    #[cfg(not(feature = "ares"))]
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
        let local = sim.intern("Local");
        let remote = sim.intern("Remote");
        let sw_type = sim.intern(name);
        let launched = |owner| SimSoundEvent::SuperWeaponLaunched {
            owner,
            sw_type,
            world: crate::sim::components::DriveCoord::cell(40, 40, 0).into(),
        };
        let radar = |rx, ry| SimSoundEvent::RadarEvent {
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
            None,
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
    /// sound first, is not compared. The mute rows (`0x00A8B538` set) defeat
    /// the current house, VERA's reading of that byte. The player's charged
    /// case 0 rows that find no silo pin the `nuke.rs` RESIDUAL: native drops
    /// the Ready line, VERA runs no tail.
    #[test]
    fn launch_lines_match_native() {
        let rules = crate::rules::ruleset::RuleSet::from_ini_with_sounds_for_test(
            &crate::rules::ini_parser::IniFile::from_str(
                "[General]\nFixtureOnly=1\n[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
                 [BuildingTypes]\n[SuperWeaponTypes]\n0=IronCurtainSpecial\n\
                 1=GeneticConverterSpecial\n2=PsychicRevealSpecial\n3=NukeSpecial\n\
                 [IronCurtainSpecial]\nType=IronCurtain\n\
                 [GeneticConverterSpecial]\nType=GeneticConverter\n\
                 [PsychicRevealSpecial]\nType=PsychicReveal\n\
                 [NukeSpecial]\nType=MultiMissile\n\
                 [AudioVisual]\nGeneticMutatorActivateSound=GeneticMutatorActivate\n\
                 PsychicRevealActivateSound=PsychicRevealActivate\nDigSound=NukeSiren\n",
            ),
            &[
                "GeneticMutatorActivate",
                "PsychicRevealActivate",
                "NukeSiren",
            ],
        )
        .unwrap();
        let oracle: serde_json::Value =
            serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json"))
                .unwrap();
        for (section, name, sound, expected) in [
            // Installed Ares hands MultiMissile to SW_NuclearMissile, whose
            // cue and line are its own facts (`ares_stock_types_tests`).
            #[cfg(not(feature = "ares"))]
            ("nuke_launch", "NukeSpecial", "NukeSiren", 12),
            ("iron_curtain_launch", "IronCurtainSpecial", "", 18),
            #[cfg(not(feature = "ares"))]
            (
                "genetic_launch",
                "GeneticConverterSpecial",
                "GeneticMutatorActivate",
                18,
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
                let events = row["events"].as_array().unwrap();
                // Case 0 launches only with a silo to fly the missile.
                let launches = row["charged"] == true
                    && (section != "nuke_launch"
                        || events.iter().any(|event| event[0] == "queue_mission"));
                // nuke.rs RESIDUAL: a charged player's launch without one
                // still drops the Ready line natively; VERA reports nothing.
                let residual = row["charged"] == true && !launches && row["player"] == true;
                let mut sim = Simulation::new();
                if row["mute"] == true {
                    let local = sim.intern("Local");
                    let mut house =
                        crate::sim::house_state::HouseState::new(local, 0, None, true, 0, 10);
                    house.set_is_defeated_for_test(true);
                    sim.houses_mut_for_test().register(house);
                    sim.session_mut_for_test()
                        .set_current_house_for_test(Some(local));
                }
                let owner = sim.intern(if row["player"] == true {
                    "Local"
                } else {
                    "Remote"
                });
                let sw_type = sim.intern(name);
                let launched = launches.then_some(SimSoundEvent::SuperWeaponLaunched {
                    owner,
                    sw_type,
                    world: crate::sim::components::DriveCoord::cell(40, 40, 0).into(),
                });
                let mut output = SoundEventQueue::new();
                dispatch_sim_sound_events(
                    launched,
                    &sim,
                    &rules,
                    Some("LOCAL"),
                    &mut |_| true,
                    &mut output,
                    None,
                );
                let mut lines: Vec<String> = output
                    .drain()
                    .into_iter()
                    .flat_map(|event| match event {
                        // The cue is a retained 750E20 request at the target
                        // with flags 0 (`PUSH 0` at each Launch site), never
                        // a one-shot riding on the EVA event.
                        GameSoundEvent::SuperWeaponActivated {
                            eva_event,
                            sound_id,
                            source: None,
                        } if sound_id.is_empty() => eva_event
                            .map(|line| format!("eva {line}"))
                            .into_iter()
                            .collect(),
                        GameSoundEvent::VocAtCoord {
                            sound_id,
                            coordinate,
                            flags: 0,
                            ..
                        } if coordinate == [40 * 256 + 128, 40 * 256 + 128, 0] => {
                            vec![format!("play {sound_id}")]
                        }
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
                    // Case 0's silo arm plays DigSound before its line
                    // (`0x006CDDE9`, `0x006CDE01`); VERA keeps one order.
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
    /// the queued Ready line through the shared tail `0x006CD51E`. Installed
    /// Ares hands SpyPlane to SW_SpyPlane (`HandlesType @ 100757F0`, hook
    /// 6CC390): its launch skips case 8, and the Ready line it drops is the
    /// common Launch's (`AresSuperWeaponFinished`), so the launch fact plays
    /// and drops nothing.
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
            let local = sim.intern("Local");
            let remote = sim.intern("Remote");
            let sw_type = sim.intern(name);
            let launched = |owner| SimSoundEvent::SuperWeaponLaunched {
                owner,
                sw_type,
                world: crate::sim::components::DriveCoord::cell(40, 40, 0).into(),
            };
            let mut output = SoundEventQueue::new();
            dispatch_sim_sound_events(
                [launched(remote), launched(local)],
                &sim,
                &rules,
                Some("LOCAL"),
                &mut |_| true,
                &mut output,
                None,
            );
            let emitted = output.drain();
            if cfg!(feature = "ares") && name == "SpyPlaneSpecial" {
                assert!(emitted.is_empty(), "{name}: {emitted:?}");
                continue;
            }
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
    fn gsi_04_07_wall_sell_sound_is_global_only_for_local_receiver() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[General]\nFixtureOnly=1\n[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
                 [AudioVisual]\nSellSound=SellBuilding\n",
            ))
            .unwrap();
        let mut sim = crate::sim::world::Simulation::new();
        let receiver = sim.intern("Receiver");
        let receiver_name = sim.interner().resolve(receiver);

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

    /// The whole `SuperClass::Launch @ 0x006CC390` cue table, one row per
    /// `Type=` case. Addresses are on `superweapon_launch_cue`.
    #[test]
    fn every_superweapon_case_plays_the_cue_its_native_arm_plays() {
        use crate::rules::superweapon_type::SuperWeaponKind as K;
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let g = retail.rules.general;
        let cue = |kind, start: Option<&str>| superweapon_launch_cue(kind, &g, start);
        let generic = cue(K::GenericWarhead, None);
        assert_eq!(generic.sound_id, None);
        assert_eq!(generic.eva_event, None);

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
        //6CEBCE..6CEBD8 retains the resolved type cue across an unknown
        // later read; exercise its actual early dispatcher and cleanup tail.
        use crate::rules::ini_parser::IniFile;
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::sound_ini::SoundRegistry;
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[SuperWeaponTypes]\n0=Shield\n[Shield]\nType=ForceShield\n\
             StartSound=forceshieldstarting\n",
        ));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[Shield]\nStartSound=MissingStart\n"),
        );
        let processed = layers.process_with_fixed_art(&IniFile::empty()).unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        let sounds =
            SoundRegistry::from_ini(&IniFile::from_str("[SoundList]\n0=ForceShieldStarting\n"));
        rules.bind_type_sound_references(processed.ini(), &sounds);
        assert_eq!(
            rules.super_weapon("Shield").unwrap().start_sound.as_deref(),
            Some("ForceShieldStarting")
        );
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        let remote = sim.intern("Remote");
        let sw_type = sim.intern("Shield");
        let world = crate::sim::anim_class::AnimWorldCoord {
            x: -257,
            y: 1537,
            z: 104,
        };
        let dispatch = |rules: &RuleSet, event| {
            let mut output = SoundEventQueue::new();
            dispatch_sim_sound_events(
                [event],
                &sim,
                rules,
                Some("LOCAL"),
                &mut |_| panic!("ForceShield sound has no radar admission"),
                &mut output,
                None,
            );
            output.drain()
        };
        // The first case uses the retained reader result. Empty and absent
        // boundary inputs stay silent through the shared cue owner.
        for cue in [Some("ForceShieldStarting"), None, Some("  ")] {
            rules.super_weapons.values_mut().next().unwrap().start_sound = cue.map(str::to_owned);
            let early = dispatch(
                &rules,
                SimSoundEvent::SuperWeaponActivation { sw_type, world },
            );
            if cue == Some("ForceShieldStarting") {
                assert!(matches!(early.as_slice(), [GameSoundEvent::VocAt {
                    sound_id, source: Some(_),
                }] if sound_id == "ForceShieldStarting"));
            } else {
                assert!(early.is_empty());
            }
            for owner in [local, remote] {
                let tail = dispatch(
                    &rules,
                    SimSoundEvent::SuperWeaponLaunched {
                        owner,
                        sw_type,
                        world,
                    },
                );
                if owner == local {
                    assert!(
                        matches!(tail.as_slice(), [GameSoundEvent::EvaRemove { event }]
                        if event == "EVA_ForceShieldReady")
                    );
                } else {
                    assert!(tail.is_empty());
                }
            }
        }
    }

    /// Ares PrintMessage callers: Ready (6CBDD7) and the ClickFire funds
    /// refusal only for the local owner, Launch (10032ED0) on every client
    /// with the owner as firer, a drain refusal (100358A0) in the player's
    /// colour wherever the sim announced it.
    #[cfg(feature = "ares")]
    #[test]
    fn ares_superweapon_message_lines_follow_their_printing_hooks() {
        use crate::app::input::messages::HudMessage;
        use crate::rules::superweapon_type::AresSuperWeaponMessage as M;
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[SuperWeaponTypes]\n0=Cell\n[Cell]\nType=Battery\n",
        ))
        .unwrap();
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        let remote = sim.intern("Remote");
        let sw_type = sim.intern("Cell");
        let events = [
            SimSoundEvent::SuperWeaponReady {
                owner: local,
                sw_type,
            },
            SimSoundEvent::SuperWeaponReady {
                owner: remote,
                sw_type,
            },
            SimSoundEvent::SuperWeaponInsufficientFunds {
                owner: remote,
                sw_type,
            },
            SimSoundEvent::AresSuperWeaponFinished {
                owner: remote,
                sw_type,
                message: true,
            },
            SimSoundEvent::AresSuperWeaponDrainRefused {
                owner: remote,
                sw_type,
            },
        ];
        let line = |message, firer| HudMessage::AresSuperWeaponMessage {
            sw_type,
            message,
            firer,
        };
        assert_eq!(
            ares_superweapon_messages(&events, &sim, &rules, Some("Local")),
            [
                line(M::Ready, Some(local)),
                line(M::Launch, Some(remote)),
                line(M::InsufficientFunds, Some(local)),
            ]
        );
    }

    /// Ares `10013E40`: the local loser reads `Message.Lost` and the local
    /// captor `Message.Capture`; another client's capture and an ordinary
    /// building print nothing.
    #[cfg(feature = "ares")]
    #[test]
    fn ares_tech_capture_labels_print_for_the_local_sides() {
        use crate::app::input::messages::HudMessage;
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        let remote = sim.intern("Remote");
        let lost = sim.intern("MSG:OilLost");
        let captured = sim.intern("MSG:OilCaptured");
        let capture = |old_owner, new_owner, tech_building| SimSoundEvent::BuildingCaptured {
            old_owner,
            new_owner,
            tech_building,
            radar: None,
            capture_eva_event: None,
            ares_tech: crate::sim::world::AresTechCaptureLines {
                lost_eva_event: None,
                message_lost: Some(lost),
                message_capture: Some(captured),
            },
        };
        let events = [
            capture(local, remote, true),
            capture(remote, local, true),
            capture(remote, remote, true),
            capture(local, remote, false),
        ];
        assert_eq!(
            ares_tech_capture_messages(&events, &sim, Some("Local")),
            [
                HudMessage::AresTechCaptureLabel(lost),
                HudMessage::AresTechCaptureLabel(captured),
            ]
        );
    }

    /// ClickFire's refusal lines (`LightningStorm::PrintMessage @
    /// 0x0053AE00`, `PsyDom::PrintMessage @ 0x0053B410`) post only on the
    /// client whose player owns the refused Super (Fire_SW passes `this ==
    /// PlayerPtr`, `0x004FAE8E`); the dispatch plays nothing for them, as
    /// the line's AddMessage plays IncomingMessage.
    #[test]
    fn a_refused_super_tells_only_its_own_player() {
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        let other = sim.intern("Other");
        #[cfg(feature = "ares")]
        let sw_type = sim.intern("Strike");
        let events = [
            SimSoundEvent::LightningStormRefused { owner: local },
            SimSoundEvent::PsychicDominatorRefused { owner: other },
            SimSoundEvent::PsychicDominatorRefused { owner: local },
            SimSoundEvent::LightningStormRefused { owner: other },
            #[cfg(feature = "ares")]
            SimSoundEvent::AresSuperWeaponRefused {
                owner: local,
                sw_type,
            },
            #[cfg(feature = "ares")]
            SimSoundEvent::AresSuperWeaponRefused {
                owner: other,
                sw_type,
            },
        ];
        assert_eq!(
            hud_messages(&events, sim.interner(), Some("Local")),
            [
                HudMessage::SuperWeapon("Msg:LightningStormActive"),
                HudMessage::SuperWeapon("Msg:DominatorActive"),
                #[cfg(feature = "ares")]
                HudMessage::AresSuperWeaponRefused(sw_type),
            ]
        );
        assert!(hud_messages(&events, sim.interner(), None).is_empty());
        let mut output = SoundEventQueue::new();
        dispatch_sim_sound_events(
            events.clone(),
            &sim,
            &dispatch_rules(),
            Some("Local"),
            &mut |_| panic!("a refusal is not a radar event"),
            &mut output,
            None,
        );
        assert!(output.drain().is_empty());
    }

    /// MPlayer_Defeated posts its line in both branches (`0x004FC271`,
    /// `0x004FC34F`) and plays `EVA_YouHaveLost` for the local House
    /// (`0x004FC2EA`), `EVA_PlayerDefeated` for any other (`0x004FC3BC`).
    /// A same-frame Flag_To_Lose line is the duplicate VoxQueue drops
    /// (`audio::vox` `duplicate_rule_matches_entry_and_type`).
    #[test]
    fn a_defeat_names_the_house_and_tells_the_local_player_they_lost() {
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        let other = sim.intern("Other");
        let events = [
            SimSoundEvent::PlayerDefeated { house: other },
            SimSoundEvent::PlayerDefeated { house: local },
        ];
        assert_eq!(
            hud_messages(&events, sim.interner(), Some("Local")),
            [
                HudMessage::PlayerDefeated(other),
                HudMessage::PlayerDefeated(local)
            ]
        );
        let mut output = SoundEventQueue::new();
        dispatch_sim_sound_events(
            events,
            &sim,
            &dispatch_rules(),
            Some("Local"),
            &mut |_| panic!("a defeat is not a radar event"),
            &mut output,
            None,
        );
        let evas: Vec<String> = output
            .drain()
            .into_iter()
            .filter_map(|event| match event {
                GameSoundEvent::Eva { event, .. } => Some(event),
                _ => None,
            })
            .collect();
        assert_eq!(evas, ["EVA_PlayerDefeated", "EVA_YouHaveLost"]);
    }

    /// Building44693D uses registry ordinal, while Ready6CBE68 and
    /// Launch6CC3AC use the unassigned Type and emit no matching case cue.
    #[test]
    fn initial_unknown_super_uses_ordinal_detection_and_keeps_selection_after_click() {
        use crate::app::input::state::MatchInputState;
        use crate::app::input::superweapon_cameo::{self, SuperWeaponClick};
        use crate::app::types::TargetingMode;
        use crate::rules::ini_parser::IniFile;
        use crate::sim::command::{Command, CommandEnvelope};
        use crate::sim::scenario_session::ScenarioGameMode;
        use crate::sim::superweapon::{SuperWeaponInstance, superweapon_views_for_owner};
        use crate::sim::world::{FrameEffects, TickLane};
        use crate::ui::sidebar::gadget_flash::SidebarGadgetState;

        fn frame(sim: &mut Simulation, rules: &RuleSet) -> Vec<SimSoundEvent> {
            let commands = sim.take_due_commands();
            sim.advance_app_frame(
                &commands,
                rules,
                None,
                67,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap()
            .sound_events
        }
        fn hear(
            events: Vec<SimSoundEvent>,
            sim: &Simulation,
            rules: &RuleSet,
        ) -> Vec<GameSoundEvent> {
            let mut output = SoundEventQueue::new();
            dispatch_sim_sound_events(
                events,
                sim,
                rules,
                Some("Americans"),
                &mut |_| false,
                &mut output,
                None,
            );
            output.drain()
        }

        let ini = crate::sim::arena_fixture::clear_ground_ini(
            "[Countries]\n0=Americans\n1=Russians\n\
             [BuildingTypes]\n0=PROVIDER\n\
             [PROVIDER]\nStrength=1000\nTurret=no\nSight=0\nSuperWeapon=Blank\n\
             [SuperWeaponTypes]\n0=Blank\n\
             [Blank]\nType=NoSuchKind\nRechargeTime=1\nIsPowered=no\nShowTimer=yes\nAction=IronCurtain\nSidebarImage=EMPTYICO\n",
        );
        let rules = RuleSet::from_ini_with_fixed_art_for_test(
            &ini,
            &IniFile::from_str("[PROVIDER]\nFoundation=1x1\n"),
        )
        .unwrap();
        assert_eq!(rules.super_weapon("Blank").unwrap().kind.native_index(), -1);
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 31);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.session_mut_for_test()
            .set_game_mode(ScenarioGameMode::Skirmish);
        sim.session_mut_for_test()
            .game_options_mut_for_test()
            .super_weapons = true;
        let local = sim.interner().get("Americans").unwrap();
        let enemy = sim.interner().get("Russians").unwrap();
        let id = sim.interner().get("Blank").unwrap();
        sim.houses_mut_for_test()
            .get_mut(&local)
            .unwrap()
            .set_player_control_for_test(true);
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(local));
        // The client fixture supplies the normal House-constructor registry;
        // grants, Ready, completion facts and clicks use production owners.
        for owner in [local, enemy] {
            sim.super_weapons_mut_for_test()
                .entry(owner)
                .or_default()
                .insert(id, SuperWeaponInstance::new(id, owner, 0));
        }
        sim.spawn_object("PROVIDER", "Americans", 8, 8, 0, &rules)
            .unwrap();
        sim.take_sound_events_for_test();
        // Unlimbo discovery opens this computer-owned building and emits the
        // completion fact. Keep it; another GrandOpening would return early.
        // The finished spawn fixture does not compare factory/buildup timing.
        sim.spawn_object("PROVIDER", "Russians", 24, 24, 0, &rules)
            .unwrap();
        let events = sim.take_sound_events_for_test();
        assert!(events.iter().any(|event| matches!(event,
            SimSoundEvent::SuperWeaponDetected { owner, sw_type } if *owner == enemy && *sw_type == id)));
        let heard = hear(events, &sim, &rules);
        let lines: Vec<_> = heard
            .iter()
            .filter_map(|event| match event {
                GameSoundEvent::Eva { event, .. } => Some(event.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            lines,
            ["EVA_NuclearSiloDetected"],
            "ordinal0 detection is independent of Type=-1"
        );

        for _ in 0..8 {
            frame(&mut sim, &rules);
        }
        let instance = &sim.super_weapons()[&local][&id];
        assert!(instance.is_active() && !instance.is_ready());
        let deadline = instance
            .charge_start_tick()
            .wrapping_add(instance.charge_duration())
            .wrapping_add(1);
        sim.session_mut_for_test()
            .set_binary_frame_for_test(deadline as u32);
        let events = frame(&mut sim, &rules);
        assert!(events.iter().any(|event| matches!(event,
            SimSoundEvent::SuperWeaponReady { owner, sw_type } if *owner == local && *sw_type == id)));
        assert!(
            hear(events, &sim, &rules).is_empty(),
            "Type=-1 has no ready line"
        );

        let view = superweapon_views_for_owner(&sim, &rules, &local)
            .pop()
            .unwrap();
        assert!(view.can_fire);
        let mut input = MatchInputState::new(Default::default());
        let mut gadgets = SidebarGadgetState::default();
        assert_eq!(
            superweapon_cameo::press(
                &mut input,
                &mut gadgets,
                Some(&view),
                rules.super_weapon("Blank").unwrap().action.as_deref(),
                false
            ),
            SuperWeaponClick::Target
        );
        let selection = input.targeting_mode.clone();
        assert_eq!(selection, Some(TargetingMode::SuperWeapon("Blank".into())));
        sim.queue_command(CommandEnvelope::new(
            local,
            sim.session().tick() + 1,
            Command::LaunchSuperWeapon {
                sw_type_id: id,
                target_rx: 20,
                target_ry: 20,
            },
        ));
        let events = frame(&mut sim, &rules);
        let instance = &sim.super_weapons()[&local][&id];
        assert!(
            instance.is_active() && !instance.is_ready(),
            "the ordinary queued click was consumed"
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, SimSoundEvent::SuperWeaponLaunched { .. }))
        );
        crate::app::match_runtime::super_selection::follow_selection_writes(
            &mut input.targeting_mode,
            &events,
            &sim,
            &rules,
            Some("Americans"),
        );
        assert_eq!(
            input.targeting_mode, selection,
            "default Launch has no selection-clear tail"
        );
        assert!(
            hear(events, &sim, &rules).is_empty(),
            "no launch cue or queued-Ready removal"
        );
    }

    #[cfg(feature = "ares")]
    #[test]
    fn construction_detection_uses_the_shared_extension_building_gate() {
        use crate::rules::ini_parser::IniFile;
        let ini = crate::sim::arena_fixture::clear_ground_ini(
            "[BuildingTypes]\n0=AUX\n1=VETO\n[AUX]\nStrength=100\n[VETO]\nStrength=100\n[SuperWeaponTypes]\n0=Gate\n[Gate]\nType=PsychicReveal\nSW.AuxBuildings=AUX\nSW.NegBuildings=VETO\n",
        );
        let rules = RuleSet::from_ini_with_fixed_art_for_test(
            &ini,
            &IniFile::from_str("[AUX]\nFoundation=1x1\n[VETO]\nFoundation=1x1\n"),
        )
        .unwrap();
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 23);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        let owner = sim.interner().get("Russians").unwrap();
        let sw_type = sim.interner().get("Gate").unwrap();
        let hear = |sim: &Simulation| {
            let mut output = SoundEventQueue::new();
            dispatch_sim_sound_events(
                [SimSoundEvent::SuperWeaponDetected { owner, sw_type }],
                sim,
                &rules,
                Some("Americans"),
                &mut |_| false,
                &mut output,
                None,
            );
            output.drain()
        };
        assert!(hear(&sim).is_empty());
        sim.spawn_object_at_height("AUX", "Russians", 6, 6, 0, 0, &rules)
            .unwrap();
        assert!(
            matches!(hear(&sim).as_slice(), [GameSoundEvent::Eva { event, .. }] if event == "EVA_NuclearSiloDetected")
        );
        sim.spawn_object_at_height("VETO", "Russians", 8, 8, 0, 0, &rules)
            .unwrap();
        assert!(hear(&sim).is_empty());
    }

    #[test]
    fn low_power_advice_posts_its_local_message_in_event_order() {
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        let other = sim.intern("Other");
        let events = [
            SimSoundEvent::LightningStormBegan,
            SimSoundEvent::HouseEva {
                owner: other,
                event: crate::sim::house_eva::EVA_LOW_POWER,
            },
            SimSoundEvent::HouseEva {
                owner: local,
                event: crate::sim::house_eva::EVA_LOW_POWER,
            },
            SimSoundEvent::HouseEva {
                owner: local,
                event: crate::sim::house_eva::EVA_INSUFFICIENT_FUNDS,
            },
            SimSoundEvent::PsychicDominatorRefused { owner: local },
        ];
        assert_eq!(
            hud_messages(&events, sim.interner(), Some("local")),
            [
                HudMessage::SuperWeapon("TXT_LIGHTNING_STORM"),
                HudMessage::LowPower,
                HudMessage::SuperWeapon("Msg:DominatorActive")
            ],
            "House4F8D14's admitted EVA also posts4F8D5C's localized row"
        );
    }

    /// Stock rules, supplied flat base, and the bound production frame API.
    /// Admission, power invalidation, guard rearm and Selling all run through
    /// their live owners; no test writes warning guards or retained power.
    #[test]
    fn house_low_power_warning_rearms_after_power_restoration_and_sale() {
        use crate::sim::command::{Command, CommandEnvelope};
        use crate::sim::house_state::HouseState;
        use crate::sim::runtime::{SimResources, SimRuntime};
        use crate::sim::world::{FrameEffects, TickLane};

        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        crate::map::theater::load_theater(&mut assets, "TEMPERATE")
            .expect("physical buildup assets for the ordinary sale");
        let mut rules = retail.rules;
        rules.bind_building_buildup_assets(&assets, "TEMPERATE");
        assert!(
            rules.has_buildup("GAPOWR"),
            "Sell_Back requires bound buildup art"
        );
        let mut sim = Simulation::with_seed(0x4F8D_005C);
        // This is the stock Battle fixture. The bare constructor's campaign
        // mode requires discovery receipts before controlled building services.
        sim.session_mut_for_test()
            .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.intern("Americans");
        let mut house = HouseState::new(owner, 0, Some(owner), true, 50_000, 10);
        house.project_country_mults(&rules, sim.interner());
        sim.houses_mut_for_test().register(house);
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        for (name, x) in [("GACNST", 4), ("GAPILE", 11)] {
            sim.spawn_object(name, "Americans", x, 4, 0, &rules)
                .expect("supplied live stock base");
        }
        let mut runtime = SimRuntime {
            simulation: sim,
            resources: SimResources {
                rules,
                overlay_registry: crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
                    &retail.processed_rules,
                    Some(&retail.fixed_art),
                ),
                ..SimResources::empty()
            }.into(),
        };
        let frame = |runtime: &mut SimRuntime, commands: &[CommandEnvelope]| {
            let output = runtime
                .advance_frame(commands, 67, TickLane::Ordinary, FrameEffects::default())
                .unwrap();
            let sim = &runtime.simulation;
            let hash = sim.state_hash();
            let messages = hud_messages(&output.sound_events, sim.interner(), Some("Americans"));
            assert!(hud_messages(&output.sound_events, sim.interner(), Some("Other")).is_empty());
            let mut sounds = SoundEventQueue::new();
            dispatch_sim_sound_events(
                output.sound_events,
                sim,
                &runtime.resources.rules,
                Some("Americans"),
                &mut |_| false,
                &mut sounds,
                None,
            );
            let voice_count = sounds
                .drain()
                .iter()
                .filter(|event| {
                    matches!(event, GameSoundEvent::Eva { event, .. }
                    if event == crate::sim::house_eva::EVA_LOW_POWER)
                })
                .count();
            assert_eq!(
                messages.len(),
                voice_count,
                "voice and text share admission"
            );
            assert!(
                messages
                    .iter()
                    .all(|message| *message == HudMessage::LowPower)
            );
            assert_eq!(
                sim.state_hash(),
                hash,
                "presentation cannot change simulation"
            );
            messages.len()
        };
        let first: usize = (0..4).map(|_| frame(&mut runtime, &[])).sum();
        assert_eq!(first, 1, "one warning for the initial shortage");
        let plant = runtime
            .simulation
            .spawn_object("GAPOWR", "Americans", 18, 4, 0, &runtime.resources.rules)
            .expect("live stock plant restores power");
        assert_eq!((0..4).map(|_| frame(&mut runtime, &[])).sum::<usize>(), 0);
        let sell = CommandEnvelope::new(
            owner,
            runtime.simulation.session().tick(),
            Command::SellBuilding { entity_id: plant },
        );
        let mut second = frame(&mut runtime, &[sell]);
        for _ in 0..180 {
            second += frame(&mut runtime, &[]);
        }
        assert_eq!(
            second, 1,
            "Selling the restored plant admits one new warning"
        );
        assert!(
            runtime.simulation.entities().get(plant).is_none(),
            "sale completed"
        );
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
            hud_messages(&events, sim.interner(), None),
            [
                HudMessage::SuperWeapon("TXT_LIGHTNING_STORM"),
                HudMessage::SuperWeapon("TXT_LIGHTNING_STORM_APPROACHING")
            ]
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
                None,
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

    /// VERA's accessibility rows ride the very same gates as the EVA voice:
    /// a remote owner's building and a refused radar event must not produce
    /// text the player never heard.
    #[test]
    fn eva_text_facts_follow_the_voice_gates() {
        use crate::sim::radar::{RadarEventRequest, RadarEventType};

        let rules = dispatch_rules();
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        let remote = sim.intern("Remote");
        let building = sim.intern("GAPOWR");
        let unit = sim.intern("HTNK");
        #[cfg(feature = "ares")]
        let promoted_eva = sim.intern("EVA_UnitPromoted");
        sim.session_mut_for_test()
            .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(local));
        let radar = RadarEventRequest::new(RadarEventType::UnitReady, 15, 15);
        let admitted_events = [
            SimSoundEvent::BuildingComplete {
                owner: remote,
                type_id: building,
            },
            SimSoundEvent::BuildingComplete {
                owner: local,
                type_id: building,
            },
            SimSoundEvent::UnitComplete {
                owner: local,
                radar,
                type_id: unit,
            },
            // A remote player's promotion is inaudible, so it must not post a
            // row either. `sound_id: None` still plays the EVA voice natively.
            SimSoundEvent::UnitPromoted {
                owner: remote,
                sound_id: None,
                type_id: unit,
                world: crate::sim::anim_class::AnimWorldCoord { x: 0, y: 0, z: 0 },
                #[cfg(feature = "ares")]
                eva: Some(promoted_eva),
            },
            SimSoundEvent::UnitPromoted {
                owner: local,
                sound_id: None,
                type_id: unit,
                world: crate::sim::anim_class::AnimWorldCoord { x: 0, y: 0, z: 0 },
                #[cfg(feature = "ares")]
                eva: Some(promoted_eva),
            },
        ];
        let mut facts = Vec::new();
        let mut output = SoundEventQueue::new();
        let mut admit = |_| true;
        dispatch_sim_sound_events(
            admitted_events,
            &sim,
            &rules,
            Some("Local"),
            &mut admit,
            &mut output,
            Some(&mut facts),
        );
        assert_eq!(
            facts,
            vec![
                HudMessage::EvaObjectLine {
                    label: "VERA:EvaConstructionComplete",
                    type_id: building
                },
                HudMessage::EvaObjectLine {
                    label: "VERA:EvaUnitReady",
                    type_id: unit
                },
                HudMessage::EvaObjectLine {
                    label: "VERA:EvaUnitPromoted",
                    type_id: unit
                },
            ]
        );

        // The kind-6 radar refusal suppresses both the voice and the row.
        let mut refused_facts = Vec::new();
        let mut refused_output = SoundEventQueue::new();
        let mut refuse = |_| false;
        dispatch_sim_sound_events(
            [SimSoundEvent::UnitComplete {
                owner: local,
                radar,
                type_id: unit,
            }],
            &sim,
            &rules,
            Some("Local"),
            &mut refuse,
            &mut refused_output,
            Some(&mut refused_facts),
        );
        assert!(refused_facts.is_empty());
        assert!(refused_output.drain().is_empty());
    }

    /// Every L2 object row names the sim's already-resolved type and appears
    /// only on the arm that admitted its EVA voice. An event with no VERA
    /// template keeps the native voice but posts no row.
    #[test]
    fn eva_object_rows_follow_each_voice_gate() {
        use crate::sim::radar::{RadarEventRequest, RadarEventType};

        let rules = dispatch_rules();
        let mut sim = Simulation::new();
        let local = sim.intern("Local");
        let remote = sim.intern("Remote");
        let building = sim.intern("GAPOWR");
        let unit = sim.intern("E1");
        #[cfg(feature = "ares")]
        let lost_eva = sim.intern("EVA_UnitLost");
        sim.session_mut_for_test()
            .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(local));
        let radar = RadarEventRequest::new(RadarEventType::UnitLost, 15, 15);
        let events = [
            SimSoundEvent::UnitLost {
                owner: local,
                radar,
                type_id: unit,
                #[cfg(feature = "ares")]
                eva: lost_eva,
                #[cfg(feature = "ares")]
                radar_event: true,
            },
            SimSoundEvent::Repairing {
                owner: local,
                type_id: building,
            },
            SimSoundEvent::StructureSold {
                owner: local,
                type_id: building,
            },
            SimSoundEvent::StructureGarrisoned {
                owner: local,
                type_id: building,
            },
            SimSoundEvent::StructureAbandoned {
                owner: local,
                radar,
                type_id: building,
            },
        ];
        let row = |label, type_id| HudMessage::EvaObjectLine { label, type_id };
        let mut facts = Vec::new();
        let mut output = SoundEventQueue::new();
        let mut admit = |_| true;
        dispatch_sim_sound_events(
            events,
            &sim,
            &rules,
            Some("Local"),
            &mut admit,
            &mut output,
            Some(&mut facts),
        );
        assert_eq!(
            facts,
            vec![
                row("VERA:EvaUnitLost", unit),
                row("VERA:EvaRepairing", building),
                row("VERA:EvaStructureSold", building),
                row("VERA:EvaStructureGarrisoned", building),
                row("VERA:EvaStructureAbandoned", building),
            ]
        );
        assert_eq!(
            output.drain().len(),
            5,
            "each admitted row still plays its native voice"
        );

        // A refused owner or radar request yields neither voice nor row; an
        // event with no VERA template keeps its voice but posts no row.
        let mut refused = Vec::new();
        let mut refused_output = SoundEventQueue::new();
        let mut refuse = |_| false;
        dispatch_sim_sound_events(
            [
                SimSoundEvent::UnitLost {
                    owner: remote,
                    radar,
                    type_id: unit,
                    #[cfg(feature = "ares")]
                    eva: lost_eva,
                    #[cfg(feature = "ares")]
                    radar_event: true,
                },
                SimSoundEvent::UnitLost {
                    owner: local,
                    radar,
                    type_id: unit,
                    #[cfg(feature = "ares")]
                    eva: lost_eva,
                    #[cfg(feature = "ares")]
                    radar_event: true,
                },
                SimSoundEvent::StructureAbandoned {
                    owner: local,
                    radar,
                    type_id: building,
                },
            ],
            &sim,
            &rules,
            Some("Local"),
            &mut refuse,
            &mut refused_output,
            Some(&mut refused),
        );
        // Ares FootClass_Destroyed (10049080) speaks the local loss, and so
        // posts its row, whatever the radar answers.
        #[cfg(feature = "ares")]
        let expected_refused = vec![row("VERA:EvaUnitLost", unit)];
        #[cfg(not(feature = "ares"))]
        let expected_refused = Vec::new();
        assert_eq!(refused, expected_refused);
        assert_eq!(
            refused_output.drain().len(),
            expected_refused.len(),
            "a refused owner or radar request keeps neither voice nor row"
        );

        // An in-scope voice with no VERA row still speaks: `EVA_UnitRepaired`
        // is deliberately outside this batch, so it must stay audible and
        // post no text.
        let mut plain = Vec::new();
        let mut plain_output = SoundEventQueue::new();
        let mut allow = |_| true;
        dispatch_sim_sound_events(
            [SimSoundEvent::UnitRepaired {
                owner: local,
                owner_admitted: false,
                radar,
            }],
            &sim,
            &rules,
            Some("Local"),
            &mut allow,
            &mut plain_output,
            Some(&mut plain),
        );
        assert!(plain.is_empty());
        assert_eq!(
            plain_output.drain().len(),
            1,
            "an out-of-scope EVA keeps its voice"
        );
    }
}
