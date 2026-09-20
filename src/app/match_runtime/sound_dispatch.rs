//! Interpret ordered simulation sound events for the local listener.
//!
//! Owns app-side cue selection, spatial projection and ordered event expansion.
//! Simulation admission predicates remain in sim; device playback remains in audio.
//! This boundary can be exercised without constructing an AppState or audio device.

use super::eva_producers;
use crate::audio::events::{GameSoundEvent, SoundEventQueue, SoundSource};
use crate::audio::sfx::SfxPlayer;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::{SimSoundEvent, Simulation};

/// The two presentation draws used while interpreting a simulation event.
/// Production delegates to the existing player RNG; absence of that player
/// still suppresses the same random-dependent cues.
pub(super) trait SoundEventRandom {
    fn pick_index(&mut self, count: usize) -> usize;
    fn roll_percent(&mut self) -> i32;
}

impl SoundEventRandom for SfxPlayer {
    fn pick_index(&mut self, count: usize) -> usize {
        SfxPlayer::pick_index(self, count)
    }

    fn roll_percent(&mut self) -> i32 {
        SfxPlayer::roll_percent(self)
    }
}

/// Append each event's output in producer order, including multi-cue events.
/// Keep this call at the frame's original sound-publication point: feedback
/// rolls precede listener gating and must not move to playback or simulation.
///
/// `admit_radar` is the local client's `CreateRadarEvent @ 0x0065FA70`. Each
/// arm below ports a native caller that tests the local player before that
/// call, so it applies the same test first and then calls `admit_radar` at
/// most once, in producer order; the result is the rate limit on the arm's EVA
/// line. That ordering belongs to those callers, not to radar events: a caller
/// with no such test must admit unconditionally.
///
/// RESIDUAL: the owner tests here compare against the local owner's name.
/// Native `0x0050B6F0` passes any `PlayerControl` house in campaign
/// (`GameMode == 0`), which can differ from the local owner. Trigger: a
/// campaign with a second player-controlled house; effect: its radar events
/// and EVA lines are dropped. Campaign play is not supported yet.
pub(super) fn dispatch_sim_sound_events(
    events: impl IntoIterator<Item = SimSoundEvent>,
    sim: &Simulation,
    rules: &RuleSet,
    local_owner_name: Option<&str>,
    mut random: Option<&mut dyn SoundEventRandom>,
    admit_radar: &mut dyn FnMut(crate::sim::radar::RadarEventRequest) -> bool,
    output: &mut SoundEventQueue,
) {
    // Convert sim sound events to app-layer sound events for playback.
    for sim_event in events {
        let app_event: GameSoundEvent = match sim_event {
            SimSoundEvent::AnimationStarted {
                anim_id,
                sound_id,
                world,
            } => GameSoundEvent::AnimationStarted {
                anim_id,
                sound_id: sim.interner.resolve(sound_id).to_string(),
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
            SimSoundEvent::WeaponFired {
                report_sound_id,
                rx,
                ry,
            } => GameSoundEvent::WeaponFired {
                sound_id: sim.interner.resolve(report_sound_id).to_string(),
                source: Some(sound_source_at_cell(rx, ry)),
            },
            SimSoundEvent::EntityDied {
                die_sound_id,
                rx,
                ry,
            } => GameSoundEvent::EntityDestroyed {
                sound_id: sim.interner.resolve(die_sound_id).to_string(),
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
            SimSoundEvent::DockDeploy { .. } => {
                // UNCHECKED residual, deliberately silent. This variant
                // has no producer: nothing in `sim/` pushes it, and
                // `miner_tests::linked_to_pivoting_then_unloading_on_pad_arrival`
                // pins that stock unload-start emits none. No native
                // counterpart was found either — a refinery has no
                // building-side dock cue. None of `[GAREFN]`,
                // `[NAREFN]`, `[YAREFN]` carries a `StartSound=` in
                // `artmd.ini` (their `ActiveAnim`/`ActiveAnimTwo..Four`
                // /`SpecialAnim` entries are silent) or any sound key
                // in `rulesmd.ini` except one: `[YAREFN]` — the
                // deployed Slave Miner, which *is* a refinery-category
                // building — carries
                // `DeploySound=SlaveMinerUndeploy` (rulesmd 13298).
                // That is an undeploy cue, not a dock cue. The full
                // stock `DeploySound=` set is `[E1]`, `[GGI]`,
                // `[AMCV]`, `[SMCV]`, `[PCV]`, `[SMIN]`, `[YAREFN]`
                // plus the empty `[AudioVisual]` default. What is
                // audible in a retail dock cycle is the miner's own
                // `MoveSound=` (landed) and the departure cue
                // `RefineryExitSfx` (landed).
                // Trigger: none today. Player effect: none.
                // Frequency: never. Downstream risk: the variant is
                // dead sim surface; deleting it needs a `SimSoundEvent`
                // change nobody currently depends on.
                continue;
            }
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
                let owner_str = sim.interner.resolve(owner);
                if !local_owner_name.is_some_and(|local| local.eq_ignore_ascii_case(owner_str)) {
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
            SimSoundEvent::BuildingComplete { owner } => {
                // Only play EVA for the local player's production.
                let owner_str = sim.interner.resolve(owner);
                if !local_owner_name.map_or(false, |l| l.eq_ignore_ascii_case(owner_str)) {
                    continue;
                }
                // `StripClass::AI 0x006A8E2F`: `PlayEVA` with type -1.
                GameSoundEvent::Eva {
                    event: "EVA_ConstructionComplete".to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::SuperWeaponLaunched {
                sw_type, rx, ry, ..
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
                if cue.sound_id.is_none() && eva_event.is_none() {
                    continue;
                }
                GameSoundEvent::SuperWeaponActivated {
                    sound_id: cue.sound_id.unwrap_or_default(),
                    source: cue.positional.then(|| sound_source_at_cell(rx, ry)),
                    eva_event,
                }
            }
            SimSoundEvent::LightningStormBegan => {
                // The deferred half of case 2: the sky flips to Ion and
                // `LightningStorm::Start` plays `StormSound`
                // (`0x0053A044`). Nothing else — the EVA line was
                // already spoken at launch, ~250 frames earlier.
                let Some(sound_id) = lightning_storm_begin_cue(&rules.general) else {
                    continue;
                };
                GameSoundEvent::SuperWeaponActivated {
                    sound_id,
                    // `PlayAtPos` with pan `0x2000`: centred, not at
                    // the storm cell.
                    source: None,
                    eva_event: None,
                }
            }
            SimSoundEvent::SuperWeaponStrike { rx, ry } => {
                // `LightningStorm::GroundStrike @
                // 0x0053A45F..0x0053A4A2`: an empty `LightningSounds=`
                // list plays nothing (`0x0053A46A TEST ECX,ECX ; JLE`),
                // otherwise one entry is drawn and played at the strike
                // coordinate through `VocClass::PlayAt @ 0x007509E0`.
                let choices = &rules.general.lightning_sounds;
                if choices.is_empty() {
                    continue;
                }
                // DRIFT, recorded: native's index comes from the
                // SCENARIO RNG (`0x0053A46E MOV EDX,[g_ScenarioClass
                // @ 0x00A8B230]; LEA ECX,[EDX+0x218]; CALL 0x0065C780`,
                // then `0x0053A48D DIV [Rules+0x744]`), so gamemd
                // spends one deterministic draw per bolt. VERA draws
                // from the presentation RNG instead, the same one
                // `audio::sfx` already uses for sample selection.
                // Trigger: every lightning bolt. Player effect: none on
                // retail — stock `LightningSounds=WeatherStrike` is a
                // one-entry list, so `rand % 1` is 0 either way and the
                // same cue plays. Frequency: several bolts per storm.
                // Downstream risk: gamemd spends one scenario draw
                // per bolt that VERA never spends, so VERA's scenario
                // stream runs one draw BEHIND from the first bolt
                // onward. On stock data VERA spends no draw at all —
                // `pick_index` short-circuits at `count <= 1` — and a
                // modded multi-entry list would also pick a different
                // entry.
                // Closing it means selecting the entry in `sim/` and
                // re-baselining whatever goldens the extra draw moves.
                let index = match random.as_deref_mut() {
                    Some(player) => player.pick_index(choices.len()),
                    None => continue,
                };
                GameSoundEvent::LightningStrike {
                    sound_id: choices[index].clone(),
                    source: Some(sound_source_at_cell(rx, ry)),
                }
            }
            SimSoundEvent::UnitComplete { owner, radar } => {
                let owner_str = sim.interner.resolve(owner);
                if !local_owner_name.map_or(false, |l| l.eq_ignore_ascii_case(owner_str)) {
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
                let owner_str = sim.interner.resolve(owner);
                if !local_owner_name.is_some_and(|local| local.eq_ignore_ascii_case(owner_str)) {
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
                let owner_str = sim.interner.resolve(owner);
                if !local_owner_name.map_or(false, |l| l.eq_ignore_ascii_case(owner_str)) {
                    continue;
                }
                // `UnitClass::Deploy 0x0073950A`: type -1.
                GameSoundEvent::Eva {
                    event: "EVA_CannotDeployHere".to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::StructureGarrisoned { owner } => {
                // EVA cue: only play for the local human player.
                let owner_str = sim.interner.resolve(owner);
                if !local_owner_name.map_or(false, |l| l.eq_ignore_ascii_case(owner_str)) {
                    continue;
                }
                // `BuildingClass::AddGarrisonOccupant 0x005229C1`: type -1
                // (stock entry QUEUE NORMAL).
                GameSoundEvent::Eva {
                    event: "EVA_StructureGarrisoned".to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::StructureAbandoned { owner } => {
                let owner_str = sim.interner.resolve(owner);
                if !local_owner_name.map_or(false, |l| l.eq_ignore_ascii_case(owner_str)) {
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
                let owner_str = sim.interner.resolve(owner);
                if !local_owner_name.map_or(false, |l| l.eq_ignore_ascii_case(owner_str)) {
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
            SimSoundEvent::RefineryExitSfx { rx, ry } => {
                // Positional SFX from [AudioVisual] BunkerWallsDownSound.
                // Skip when rules don't configure the sound (matches
                // gamemd's `RulesClass+0x244 != -1` guard).
                let sound_id =
                    match Some(rules).and_then(|r| r.general.bunker_walls_down_sound.as_deref()) {
                        Some(s) if !s.is_empty() => s.to_string(),
                        _ => continue,
                    };
                GameSoundEvent::RefineryExitSfx {
                    sound_id,
                    source: Some(sound_source_at_cell(rx, ry)),
                }
            }
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
            SimSoundEvent::VoiceFeedback {
                owner,
                type_ref,
                rx,
                ry,
            } => {
                // `TechnoClass::ReceiveDamage @ 0x00701900`, arm
                // `0x00702695`. `sim/` has already applied the two
                // gates that are deterministic — result 2 (the
                // `Strength >> 1` crossing) and a non-empty
                // `VoiceFeedback=` list (`0x007026A1`/`0x007026A9`).
                // What is left runs in native's exact order.
                //
                // 1. `0x007026B3` `RandomRanged(0, 99)` on
                //    `g_MainRng @ 0x00886B88`, `0x007026BD CMP
                //    EAX,0x1E ; JGE` — speaks on 0..=29. Native spends
                //    this draw for every house, so it is drawn before
                //    the owner gate here too.
                let Some(player) = random.as_deref_mut() else {
                    continue;
                };
                let roll = player.roll_percent();
                // 2. `0x007026C6 MOV ECX,[ESI+0x21C]` / `CALL
                //    HouseClass::IsHumanPlayer @ 0x0050B6F0`. With
                //    `g_GameMode != 0` (skirmish/multiplayer) that
                //    function is `house == g_PlayerPtr`, so only the
                //    local player's objects speak.
                let owner_str = sim.interner.resolve(owner);
                let owner_is_local_human =
                    local_owner_name.is_some_and(|local| local.eq_ignore_ascii_case(owner_str));
                if !voice_feedback_speaks(roll, owner_is_local_human) {
                    continue;
                }
                // 3. `0x007026DE CALL 0x0065C780` / `0x007026E7 DIV
                //    [EDI+0x4E8]` picks `items[rand % count]`.
                //    RESIDUAL: VERA models `VoiceFeedback=` as one id
                //    where native holds a `CCINIClass::ReadSoundList`
                //    vector, so no draw is spent here. Trigger: every
                //    spoken line. Player effect: none on retail — all
                //    133 `VoiceFeedback=` authors in `rulesmd.ini` are
                //    single-entry (0 contain a comma), so `rand % 1`
                //    is 0 either way. Frequency: every half-health
                //    crossing that passes the roll. Downstream risk:
                //    none for lockstep — `g_MainRng` is not
                //    synchronised — but a modded comma list would pick
                //    the wrong entry. Same divergence as the
                //    `VoiceMove=` one recorded on `voice_id_for_key`.
                let sound_id = match rules
                    .object(sim.interner.resolve(type_ref))
                    .and_then(|object| object.voice_feedback.as_deref())
                {
                    Some(s) if !s.is_empty() => s.to_string(),
                    _ => continue,
                };
                GameSoundEvent::VoiceFeedback {
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
                let owner_str = sim.interner.resolve(owner);
                let is_local = local_owner_name.is_some_and(|l| l.eq_ignore_ascii_case(owner_str));
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
                let owner_str = sim.interner.resolve(owner);
                if !local_owner_name.is_some_and(|l| l.eq_ignore_ascii_case(owner_str))
                    || !admit_radar(radar)
                {
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
                let owner_str = sim.interner.resolve(owner);
                if !local_owner_name.is_some_and(|l| l.eq_ignore_ascii_case(owner_str))
                    || !admit_radar(radar)
                {
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
                let owner_str = sim.interner.resolve(owner);
                if !local_owner_name.is_some_and(|l| l.eq_ignore_ascii_case(owner_str)) {
                    continue;
                }
                GameSoundEvent::Eva {
                    event: event.to_string(),
                    type_override: None,
                }
            }
            SimSoundEvent::StructureSold { owner } => {
                // `BuildingClass::Sell 0x00449CC1 MOV AL,[EBP+0x41A]`
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
            SimSoundEvent::Repairing { owner } => {
                // `BuildingClass::ToggleRepair 0x004470A4 CALL
                // 0x0050B6F0`: the owner is the local player.
                if !owner_is_local(&sim.interner, owner, local_owner_name) {
                    continue;
                }
                GameSoundEvent::Eva {
                    event: eva_producers::EVA_REPAIRING.to_string(),
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
                let aux_satisfied = match rules
                    .super_weapon(&type_name)
                    .and_then(|sw| sw.aux_building.as_deref())
                {
                    None => true,
                    Some(aux) => sim.substrate.entities.values().any(|e| {
                        e.owner() == owner
                            && !e.dying
                            && !e.lifecycle.in_limbo
                            && e.category == crate::map::entities::EntityCategory::Structure
                            && sim.interner.resolve(e.type_ref()).eq_ignore_ascii_case(aux)
                    }),
                };
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

/// Chance in 100 that a techno speaks its `VoiceFeedback=` line on the
/// half-strength crossing. `TechnoClass::ReceiveDamage @ 0x007026BD
/// CMP EAX,0x1E ; JGE` against `RandomRanged(0, 99)` — the cue speaks for a
/// draw of 0..=29. Hardcoded in the binary, not an INI key.
const VOICE_FEEDBACK_PERCENT: i32 = 0x1E;

/// `TechnoClass::ReceiveDamage @ 0x00702695`'s two post-list gates, in native
/// order: the roll at `0x007026BD CMP EAX,0x1E ; JGE 0x007027F7` against
/// `RandomRanged(0, 99)`, then `HouseClass::IsHumanPlayer @ 0x0050B6F0` at
/// `0x007026C6` — which for `g_GameMode != 0` (skirmish and multiplayer) is
/// `house == g_PlayerPtr`, the local player alone.
///
/// The caller must have drawn `roll` already whatever the owner is: native
/// spends the draw at `0x007026B3`, before it loads `[ESI+0x21C]`.
fn voice_feedback_speaks(roll: i32, owner_is_local_human: bool) -> bool {
    roll < VOICE_FEEDBACK_PERCENT && owner_is_local_human
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
    /// [`lightning_storm_begin_cue`].
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
/// | `LightningStorm` | — (the `StormSound` cue is deferred, see [`lightning_storm_begin_cue`]) | `EVA_LightningStormCreated` (`0x006CCD81`) |
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
        // [`lightning_storm_begin_cue`] carries it.
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

/// The cue the storm plays when it *begins* — `[Rules+0x730]`, i.e.
/// `[AudioVisual] StormSound=` (key `"StormSound"` at `0x0083A400`, bound at
/// `0x0066AEAE`/`0x0066AEE6`), behind the `0x0053A014 MOV AL,[Rules+0x17B0]`
/// gate. `Rules+0x17B0` is `[General] LightningPrintText=` and the
/// `0x0053A01C JZ` skips the cue and the on-screen storm message together.
///
/// **Not a launch cue.** `SuperClass::Launch @ 0x006CC390` case 2 passes
/// `[Rules+0x1794]` (`LightningDeferment`) as `LightningStorm::Start @
/// 0x00539EB0`'s `param_2`, and `Start` opens `if (param_2 != 0) { arm the
/// countdown; return; }` — returning before `0x0053A044`. `LightningStorm::
/// Process @ 0x0053A6C0` decrements that countdown (`0x0053AAAD`) and at zero
/// re-enters `Start` with `param_2` cleared (`0x0053AAC8 XOR EDX,EDX`), which
/// is the entry that reaches the cue. Stock `rulesmd.ini:130` sets
/// `LightningDeferment=250`, so on retail the cue always trails the launch
/// EVA line by the full deferment.
///
/// Non-positional: `0x0053A044 CALL VocClass::PlayAtPos @ 0x00750920` with pan
/// `0x2000` (`0x0053A03A`) and volume `1.0f` (`0x0053A03F`), so it is played
/// centred rather than at the storm cell.
pub(crate) fn lightning_storm_begin_cue(
    general: &crate::rules::ruleset::GeneralRules,
) -> Option<String> {
    general
        .lightning_print_text
        .then(|| general.storm_sound.clone())
        .flatten()
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
             [AudioVisual]\nBaseUnderAttackSound=Siren\nLightningSounds=StrikeA,StrikeB\n",
        ))
        .unwrap()
    }

    #[derive(Default)]
    struct ScriptedRandom {
        rolls: std::collections::VecDeque<i32>,
        calls: Vec<&'static str>,
    }

    impl SoundEventRandom for ScriptedRandom {
        fn pick_index(&mut self, count: usize) -> usize {
            self.calls.push("index");
            assert_eq!(count, 2);
            1
        }

        fn roll_percent(&mut self) -> i32 {
            self.calls.push("percent");
            self.rolls.pop_front().expect("scripted percentage draw")
        }
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
            None,
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
    fn dispatcher_draws_feedback_before_listener_gate_and_keeps_rng_event_order() {
        let rules = dispatch_rules();
        let mut sim = Simulation::new();
        let local = sim.interner.intern("Local");
        let remote = sim.interner.intern("Remote");
        let type_ref = sim.interner.intern("E1");
        let feedback = |owner| SimSoundEvent::VoiceFeedback {
            owner,
            type_ref,
            rx: 3,
            ry: 4,
        };
        let mut random = ScriptedRandom {
            rolls: [0, 99, 29].into(),
            ..Default::default()
        };
        let mut output = SoundEventQueue::new();
        dispatch_sim_sound_events(
            [
                feedback(remote),
                feedback(local),
                SimSoundEvent::SuperWeaponStrike { rx: 8, ry: 9 },
                feedback(local),
            ],
            &sim,
            &rules,
            Some("Local"),
            Some(&mut random),
            &mut |_| panic!("no radar-gated event in this batch"),
            &mut output,
        );
        assert_eq!(random.calls, ["percent", "percent", "index", "percent"]);
        assert!(random.rolls.is_empty());
        let events = output.drain();
        assert_eq!(
            events
                .iter()
                .map(GameSoundEvent::sound_id)
                .collect::<Vec<_>>(),
            ["StrikeB", "Feedback"]
        );
        assert_eq!(events[0].source().unwrap().cell(), (8, 9));
        assert_eq!(events[1].source().unwrap().cell(), (3, 4));
    }

    #[test]
    fn dispatcher_without_audio_rng_keeps_nonrandom_events() {
        let rules = dispatch_rules();
        let mut sim = Simulation::new();
        let local = sim.interner.intern("Local");
        let type_ref = sim.interner.intern("E1");
        let report_sound_id = sim.interner.intern("Shot");
        let mut output = SoundEventQueue::new();
        dispatch_sim_sound_events(
            [
                SimSoundEvent::VoiceFeedback {
                    owner: local,
                    type_ref,
                    rx: 1,
                    ry: 2,
                },
                SimSoundEvent::SuperWeaponStrike { rx: 1, ry: 2 },
                SimSoundEvent::WeaponFired {
                    report_sound_id,
                    rx: 1,
                    ry: 2,
                },
            ],
            &sim,
            &rules,
            Some("Local"),
            None,
            &mut |_| panic!("no radar-gated event in this batch"),
            &mut output,
        );
        let events = output.drain();
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], GameSoundEvent::WeaponFired { sound_id, .. } if sound_id == "Shot")
        );
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

    /// `TechnoClass::ReceiveDamage @ 0x00702695`, the `VoiceFeedback=` arm:
    /// `0x007026BD CMP EAX,0x1E ; JGE 0x007027F7` against
    /// `RandomRanged(0, 99)`, then `HouseClass::IsHumanPlayer @ 0x0050B6F0`.
    #[test]
    fn the_damage_voice_speaks_on_a_roll_under_thirty_and_only_for_the_local_owner() {
        // `JGE 0x1E` — 0..=29 speak (30 in 100), 30..=99 fall to the tail.
        assert!(voice_feedback_speaks(0, true));
        assert!(voice_feedback_speaks(29, true));
        assert!(!voice_feedback_speaks(30, true));
        assert!(!voice_feedback_speaks(99, true));

        // `0x007026C6 MOV ECX,[ESI+0x21C]` / `IsHumanPlayer`: an AI's or an
        // opponent's object never speaks, however the roll landed.
        assert!(!voice_feedback_speaks(0, false));
        assert!(!voice_feedback_speaks(29, false));
        assert!(!voice_feedback_speaks(30, false));
    }

    /// The roll itself is `RandomRanged(0, 99)` (`0x007026AF PUSH 0x63 ;
    /// PUSH 0x0`), and `Random__RandomRanged @ 0x0065C7E0` only skips the draw
    /// when its two endpoints are equal — so unlike `SfxPlayer::pick_index`
    /// this one always advances the generator.
    #[test]
    fn the_damage_voice_roll_covers_the_native_range_at_the_native_rate() {
        let mut rng = crate::audio::sfx::SfxRng::seeded(0x5EED);
        let mut speaks = 0usize;
        for _ in 0..10_000 {
            let roll = crate::audio::sfx::SampleRng::ranged(&mut rng, 0, 99);
            assert!((0..=99).contains(&roll), "roll {roll} left [0, 99]");
            if roll < VOICE_FEEDBACK_PERCENT {
                speaks += 1;
            }
        }
        // 30 in 100. The band is wide on purpose: this pins what the 0x1E
        // threshold means, not the generator behind it.
        assert!((2_600..3_400).contains(&speaks), "spoke {speaks} of 10000");
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
        // launch cue — `the_storm_cue_is_deferred_and_rides_lightning_print_text`
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

    /// `StormSound` belongs to the storm *beginning*, not to the launch.
    /// `SuperClass::Launch` case 2 hands `[Rules+0x1794]`
    /// (`LightningDeferment`, stock 250) to `LightningStorm::Start @
    /// 0x00539EB0`, whose `if (param_2 != 0) { arm the countdown; return; }`
    /// returns before the cue at `0x0053A044`;
    /// `LightningStorm::Process @ 0x0053A6C0` re-enters with `param_2` cleared
    /// (`0x0053AAC8 XOR EDX,EDX`) at countdown zero and that entry plays it.
    ///
    /// `0x0053A014 MOV AL,[Rules+0x17B0]` (`[General] LightningPrintText=`)
    /// gates the cue and the on-screen storm message together. The EVA line is
    /// outside that gate — `SuperClass::Launch` plays it at `0x006CCD81`,
    /// after `Start` has returned, deferred or not.
    #[test]
    fn the_storm_cue_is_deferred_and_rides_lightning_print_text() {
        use crate::rules::superweapon_type::SuperWeaponKind as K;
        let on = stock_audio_visual();
        assert!(on.lightning_print_text, "the constructor default is true");

        // The launch moment: EVA only, no cue.
        let launch = superweapon_launch_cue(K::LightningStorm, &on, None);
        assert_eq!(launch.sound_id, None);
        assert_eq!(launch.eva_event, Some("EVA_LightningStormCreated"));

        // The beginning: the cue, and nothing else.
        assert_eq!(
            lightning_storm_begin_cue(&on).as_deref(),
            Some("WeatherIntro")
        );

        let mut off = stock_audio_visual();
        off.lightning_print_text = false;
        assert_eq!(lightning_storm_begin_cue(&off), None);
        assert_eq!(
            superweapon_launch_cue(K::LightningStorm, &off, None).eva_event,
            Some("EVA_LightningStormCreated"),
            "the EVA line is outside the LightningPrintText gate"
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
