//! Game sound events — the bridge between sim and audio.
//!
//! The simulation produces GameSoundEvents when things happen (weapon fired,
//! unit selected, entity destroyed). The app layer collects these events each
//! tick and feeds them to the SfxPlayer for playback.
//!
//! Events carry the sound ID (from rules.ini / sound.ini) rather than a
//! filename — the SfxPlayer resolves IDs to files via SoundRegistry.
//!
//! ## Design
//! Events are plain data — no audio library handles, no asset references. This keeps
//! sim/ free from audio dependencies. The event queue is a simple Vec that
//! gets drained each frame.
//!
//! ## Dependency rules
//! - Part of audio/ — but contains no rodio code, only data types.
//! - sim/ may reference this module to push events (acceptable because
//!   it's pure data with zero audio-library dependencies).

/// Where a positional sound plays: the world-pixel point the presentation
/// frame draws it at, and the cell it occupies.
///
/// gamemd-derived: `VocClass::PlayAt @ 0x007509E0` receives the object's
/// coordinate; `VocClass::CalcVolumeAndPan @ 0x00750AC0` projects it to the
/// tactical client point for volume and pan and derives the cell as
/// `coord >> 8` (`0x00750B32..0x00750B4E`) for the `Type=SHROUD` gate. VERA
/// carries both halves so the app never has to invert a projection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoundSource {
    pub screen_x: f32,
    pub screen_y: f32,
    pub rx: u16,
    pub ry: u16,
}

impl SoundSource {
    pub fn new(screen: (f32, f32), cell: (u16, u16)) -> Self {
        Self {
            screen_x: screen.0,
            screen_y: screen.1,
            rx: cell.0,
            ry: cell.1,
        }
    }

    pub fn screen_pos(&self) -> (f32, f32) {
        (self.screen_x, self.screen_y)
    }

    pub fn cell(&self) -> (u16, u16) {
        (self.rx, self.ry)
    }
}

/// A sound event produced by the game simulation or UI.
#[derive(Debug, Clone)]
pub enum GameSoundEvent {
    /// One `VoxClass::PlayEVA @ 0x00752700` call: the `[DialogList]` event
    /// name and the call site's type override (`-1` = the entry's own
    /// `Type=`; only `SelectClass::Action` OnHold/Canceled and
    /// `GameExit::BattleControlTerminated 0x00686616` pass `2`). The side
    /// column, `Type=`/`Priority=` routing, duplicate rule and the 500 ms gap
    /// are the consumer's (`SfxPlayer::play_eva` → `audio::vox`). Producers
    /// gate on the local player before pushing this.
    Eva {
        event: String,
        type_override: Option<crate::rules::sound_ini::EvaType>,
    },

    /// Start/report sound owned by one authoritative animation object.
    AnimationStarted {
        anim_id: u64,
        sound_id: String,
        source: Option<SoundSource>,
    },

    /// Release one animation's active handle, then optionally play StopSound.
    AnimationStopped {
        anim_id: u64,
        stop_sound_id: Option<String>,
        source: Option<SoundSource>,
    },
    /// A weapon fired — play the weapon's Report= sound.
    WeaponFired {
        /// sound.ini ID from the weapon's Report= field.
        sound_id: String,
        /// Screen position of the sound source (for spatial audio).
        /// If None, plays at full volume (non-positional).
        source: Option<SoundSource>,
    },

    /// A unit was selected by the player — play VoiceSelect.
    ///
    /// `speaker_id` is the object that speaks: `TechnoClass::Queue_Voice @
    /// 0x00708D90` latches the line on the techno itself (`+0x4F0`), so the
    /// repeat guard in [`crate::audio::voice_queue`] needs to know whose line
    /// this is.
    UnitSelected {
        /// Stable id of the speaking object.
        speaker_id: u64,
        /// sound.ini ID from the unit's VoiceSelect= field.
        sound_id: String,
    },

    /// A unit was ordered to move — play VoiceMove.
    UnitMoveOrder {
        /// Stable id of the speaking object.
        speaker_id: u64,
        /// sound.ini ID from the unit's VoiceMove= field.
        sound_id: String,
    },

    /// A unit was ordered to attack — play VoiceAttack.
    UnitAttackOrder {
        /// Stable id of the speaking object.
        speaker_id: u64,
        /// sound.ini ID from the unit's VoiceAttack= field.
        sound_id: String,
    },

    /// An entity was destroyed — play DieSound.
    EntityDestroyed {
        /// sound.ini ID from the entity's DieSound= field.
        sound_id: String,
        /// Screen position of the sound source (for spatial audio).
        source: Option<SoundSource>,
    },

    /// An entity was crushed by a vehicle — play CrushSound (the squish).
    EntityCrushed {
        /// sound.ini ID from the entity's CrushSound= field.
        sound_id: String,
        /// Screen position of the sound source (for spatial audio).
        source: Option<SoundSource>,
    },

    /// DeploySound on infantry stance entry or successful unit-to-building conversion.
    AircraftPhase {
        sound_id: String,
        source: Option<SoundSource>,
    },
    EntityDeployed {
        /// sound.ini ID from the entity's DeploySound= field.
        sound_id: String,
        /// Screen position of the sound source (for spatial audio).
        source: Option<SoundSource>,
    },

    /// A transport placed an ejected passenger — play the transport type's
    /// LeaveTransportSound at the transport's screen position.
    LeaveTransport {
        /// sound.ini ID from the transport type's LeaveTransportSound= field.
        sound_id: String,
        /// Screen position of the sound source (for spatial audio).
        source: Option<SoundSource>,
    },

    /// An infantry entity entered the Undeploying phase — play UndeploySound.
    EntityUndeployed {
        /// sound.ini ID from the entity's UndeploySound= field.
        sound_id: String,
        /// Screen position of the sound source (for spatial audio).
        source: Option<SoundSource>,
    },

    /// A chrono teleport happened — play the resolved warp sound at this position.
    /// Emitted twice per warp (source = ChronoOutSound, destination = ChronoInSound).
    ChronoTeleport {
        /// sound.ini ID — already resolved to the per-unit ChronoIn/OutSound by sim.
        sound_id: String,
        /// Screen position of the sound source (for spatial audio).
        source: Option<SoundSource>,
    },

    /// A local-player object crossed a veterancy rank — the positional
    /// `[AudioVisual] UpgradeVeteranSound=`/`UpgradeEliteSound=` cue
    /// (`VocClass::PlayAt` from `TechnoClass::AI_Update @ 0x006FA0BC`).
    UnitPromoted {
        sound_id: String,
        source: Option<SoundSource>,
    },

    /// One-shot positional `[AudioVisual] CloakSound` requested by an accepted
    /// native StartUncloaking arg-zero transition.
    CloakSound {
        sound_id: String,
        source: Option<SoundSource>,
    },

    /// One-shot positional overlay `CrushSound=` played by
    /// `UnitClass::PerCellProcess @ 0x0073B04D` when a crusher flattens a wall.
    WallCrushed {
        sound_id: String,
        source: Option<SoundSource>,
    },

    /// Positional SFX from [AudioVisual] BuildingGarrisonedSound — plays at
    /// the building's screen position when the first occupant enters.
    BuildingGarrisonedSfx {
        /// sound.ini ID for the SFX (resolves "BuildingGarrisoned" → file).
        sound_id: String,
        /// Screen position for spatial audio.
        source: Option<SoundSource>,
    },

    /// Positional SFX from `[SealPlaceBomb]` — plays at the attacker's screen
    /// position when a C4-capable infantry claims a plant on a CanC4 building.
    C4Planted {
        /// sound.ini ID for the SFX (resolves "SealPlaceBomb" → file).
        sound_id: String,
        /// Screen position for spatial audio.
        source: Option<SoundSource>,
    },

    /// Positional SFX played when a docked harvester departs after dumping.
    /// Resolved from [AudioVisual] BunkerWallsDownSound (retail "TankBunkerDown").
    /// Fires every refinery dock cycle.
    RefineryExitSfx {
        /// sound.ini ID for the SFX.
        sound_id: String,
        /// Screen position for spatial audio.
        source: Option<SoundSource>,
    },

    /// Positional SFX for tank-bunker walls raising (install) or falling
    /// (exit/teardown). The up/down choice — and thus which rules key resolves
    /// `sound_id` — is made when the event is built; plays at the bunker's
    /// screen position. Skipped upstream when the rules key is empty.
    BunkerWalls {
        /// sound.ini ID (BunkerWallsUpSound or BunkerWallsDownSound).
        sound_id: String,
        /// Screen position for spatial audio.
        source: Option<SoundSource>,
    },

    /// Positional SFX played when a paradropped passenger successfully opens
    /// a parachute. Resolved from [AudioVisual] ChuteSound.
    ChuteSound {
        /// sound.ini ID for the SFX.
        sound_id: String,
        /// Screen position for spatial audio.
        source: Option<SoundSource>,
    },

    /// Positional SFX + EVA cue from a bridge repair triggered by an engineer
    /// entering a `BridgeRepairHut`. Plays the spatial `[BridgeRepaired]`
    /// sound (resolved from `rules.bridge_rules.repair_sound`) at the hut's
    /// screen position when `sound_id` is non-empty. When `eva_event` is
    /// `Some`, the EVA arm plays it as a non-positional cue (gated upstream
    /// on local-human owner).
    BridgeRepaired {
        /// Spatial `[BridgeRepaired]` SFX id. Empty when
        /// `RepairBridgeSound=` is unset in rules.
        sound_id: String,
        /// Screen position for spatial audio.
        source: Option<SoundSource>,
        /// `Some("EVA_BridgeRepaired")` when the engineer's owner is the
        /// local human and the radar dedupe allowed it; `None` otherwise.
        eva_event: Option<String>,
    },

    /// The `[AudioVisual] BaseUnderAttackSound` siren that rides with the
    /// under-attack EVA line.
    ///
    /// `HouseClass::NotifyUnderAttack @ 0x004F95B8..0x004F95CF` plays
    /// `Rules+0x184` through `VocClass::PlayAtPos @ 0x00750920` with pan
    /// `0x2000` and volume `1.0f` — non-positional — immediately after the EVA
    /// call at `0x004F95B3`, and only when `CreateRadarEvent @ 0x0065FA70`
    /// accepted the ping (`0x004F95A5 TEST AL,AL ; JZ`).
    BaseUnderAttackSfx {
        /// sound.ini ID from `[AudioVisual] BaseUnderAttackSound=`.
        sound_id: String,
    },

    /// The global `[AudioVisual] BuildingDamageSound` cue for a struck
    /// building whose type carries no `DamageSound=` of its own.
    ///
    /// `BuildingClass::ReceiveDamage @ 0x00442230` plays it at the building's
    /// own coordinate (`0x004426DB LEA ECX,[ESI+0x9C]`,
    /// `0x00442700 MOV ECX,[Rules+0x714]`,
    /// `0x00442706 CALL VocClass::PlayAtCoord @ 0x00750E20`). The gate is
    /// `0x004426D2 CMP [type+0x538],-1` and the arm is only reached for the
    /// damage-state crossings 2 and 3 (`0x00442476 JMP [EAX*4 + 0x00442C18]`),
    /// both decided sim-side.
    BuildingDamagedSfx {
        /// sound.ini ID from `[AudioVisual] BuildingDamageSound=`.
        sound_id: String,
        /// Screen position for spatial audio.
        source: Option<SoundSource>,
    },

    /// A techno crossed the half-strength threshold and speaks its
    /// `VoiceFeedback=` line.
    ///
    /// `TechnoClass::ReceiveDamage @ 0x00701900`, arm `0x00702695` (index 2 of
    /// the switch table at `0x00702D24`, i.e. damage result 2). The 30% roll,
    /// the `HouseClass::IsHumanPlayer @ 0x0050B6F0` gate and the
    /// `rand % count` pick are resolved before this event is built; it plays
    /// positionally through `VocClass::PlayAt @ 0x007509E0` at the object's
    /// own coordinate.
    VoiceFeedback {
        /// sound.ini ID for the chosen `VoiceFeedback=` entry.
        sound_id: String,
        /// Screen position for spatial audio.
        source: Option<SoundSource>,
    },

    /// A lightning-storm bolt reached the ground — the thunder crack.
    ///
    /// `LightningStorm::GroundStrike @ 0x0053A45F..0x0053A4A2` plays
    /// `[AudioVisual] LightningSounds[n]` positionally at the strike cell
    /// through `VocClass::PlayAt @ 0x007509E0`; the entry is chosen when the
    /// event is built. Skipped upstream when the list is empty
    /// (`0x0053A46A TEST ECX,ECX ; JLE`).
    LightningStrike {
        /// sound.ini ID for the chosen `LightningSounds=` entry.
        sound_id: String,
        /// Screen position for spatial audio.
        source: Option<SoundSource>,
    },

    /// A superweapon fired: its `[AudioVisual]` cue and/or its EVA warning.
    ///
    /// `SuperClass::Launch @ 0x006CC390` decides both per `Type=` case; see
    /// [`crate::app::match_runtime::sound_dispatch::superweapon_launch_cue`] for the
    /// case-by-case table and its addresses. Every cue in that table is played
    /// by `VocClass::PlayAtCoord @ 0x00750E20` (or `PlayAt @ 0x007509E0` for
    /// `ForceShield`) at the target coordinate, hence `source`.
    ///
    /// The one `source: None` producer is `StormSound`, and it is not a launch
    /// cue: `LightningStorm::Start @ 0x00539EB0` returns early on a deferred
    /// launch, so the cue is played only when the deferment expires
    /// (`0x0053A044`, `VocClass::PlayAtPos @ 0x00750920`, pan `0x2000`, volume
    /// `1.0f` — centred and full-volume). See
    /// [`crate::app::match_runtime::sound_dispatch::lightning_storm_begin_cue`].
    ///
    /// The EVA line is not gated on the launching house: native calls
    /// `VoxClass::PlayEVA` behind `[0x00A8B538]` only, a client-side flag that
    /// `HouseClass::MPlayer_Defeated @ 0x004FC205` sets to 1 — i.e. an
    /// enemy's launch announces itself on your client until you are defeated.
    SuperWeaponActivated {
        /// `[AudioVisual]`/`StartSound=` cue; empty when the case plays none.
        sound_id: String,
        /// Target-cell position, or `None` for a non-positional cue.
        source: Option<SoundSource>,
        /// `evamd.ini` event name for the `VoxClass::PlayEVA` call, or `None`
        /// when the case plays no EVA line. Routing comes from the entry.
        eva_event: Option<String>,
    },

    /// Generic UI sound (button click, error beep, etc.).
    UiSound {
        /// sound.ini ID for the UI sound.
        sound_id: String,
    },

    /// One displayed-credits counter step: `[AudioVisual] CreditTicks[0]`
    /// while counting up, `[1]` while counting down. `CreditsClass::Draw @
    /// 0x004A2510..0x004A2533` plays it through `VocClass::PlayAtPos @
    /// 0x00750920` at volume `0.5f` (`PUSH 0x3f000000`), pan `0x2000`
    /// (centre), no handle — so, unlike [`Self::UiSound`], at HALF volume.
    CreditTick {
        /// sound.ini ID from the `CreditTicks=` list.
        sound_id: String,
    },
}

impl GameSoundEvent {
    /// Get the sound ID for this event.
    pub fn sound_id(&self) -> &str {
        match self {
            Self::WeaponFired { sound_id, .. }
            | Self::AnimationStarted { sound_id, .. }
            | Self::UnitSelected { sound_id, .. }
            | Self::UnitMoveOrder { sound_id, .. }
            | Self::UnitAttackOrder { sound_id, .. }
            | Self::EntityDestroyed { sound_id, .. }
            | Self::EntityCrushed { sound_id, .. }
            | Self::AircraftPhase { sound_id, .. }
            | Self::EntityDeployed { sound_id, .. }
            | Self::LeaveTransport { sound_id, .. }
            | Self::EntityUndeployed { sound_id, .. }
            | Self::ChronoTeleport { sound_id, .. }
            | Self::UnitPromoted { sound_id, .. }
            | Self::CloakSound { sound_id, .. }
            | Self::WallCrushed { sound_id, .. }
            | Self::UiSound { sound_id }
            | Self::CreditTick { sound_id }
            | Self::BaseUnderAttackSfx { sound_id }
            | Self::BuildingGarrisonedSfx { sound_id, .. }
            | Self::C4Planted { sound_id, .. }
            | Self::RefineryExitSfx { sound_id, .. }
            | Self::BunkerWalls { sound_id, .. }
            | Self::ChuteSound { sound_id, .. }
            | Self::BridgeRepaired { sound_id, .. }
            | Self::BuildingDamagedSfx { sound_id, .. }
            | Self::VoiceFeedback { sound_id, .. }
            | Self::LightningStrike { sound_id, .. }
            | Self::SuperWeaponActivated { sound_id, .. } => sound_id,
            Self::AnimationStopped { stop_sound_id, .. } => stop_sound_id.as_deref().unwrap_or(""),
            // The event name, not a sample: the sample is a per-side column
            // the `VoxClass` consumer resolves.
            Self::Eva { event, .. } => event,
        }
    }

    /// Get the positional source for spatial audio, if this event has one.
    pub fn source(&self) -> Option<SoundSource> {
        match self {
            Self::WeaponFired { source, .. }
            | Self::AnimationStarted { source, .. }
            | Self::AnimationStopped { source, .. }
            | Self::EntityDestroyed { source, .. }
            | Self::EntityCrushed { source, .. }
            | Self::AircraftPhase { source, .. }
            | Self::EntityDeployed { source, .. }
            | Self::LeaveTransport { source, .. }
            | Self::EntityUndeployed { source, .. }
            | Self::ChronoTeleport { source, .. }
            | Self::UnitPromoted { source, .. }
            | Self::CloakSound { source, .. }
            | Self::WallCrushed { source, .. }
            | Self::BuildingGarrisonedSfx { source, .. }
            | Self::C4Planted { source, .. }
            | Self::RefineryExitSfx { source, .. }
            | Self::BunkerWalls { source, .. }
            | Self::ChuteSound { source, .. }
            | Self::BridgeRepaired { source, .. }
            | Self::BuildingDamagedSfx { source, .. }
            | Self::VoiceFeedback { source, .. }
            | Self::LightningStrike { source, .. }
            | Self::SuperWeaponActivated { source, .. } => *source,
            _ => None,
        }
    }

    /// Get the screen position for spatial audio, if this event has one.
    pub fn screen_pos(&self) -> Option<(f32, f32)> {
        self.source().map(|source| source.screen_pos())
    }
}

/// Collects sound events during a simulation tick for later playback.
///
/// Drained by the app layer each frame after sim ticking.
///
/// RESIDUAL (GSI-15.05/15.08) — pass 2 split this into what is closed, what is
/// landable and what is genuinely blocked.
///
/// **NO-DIFF, retired from the list.** `DownReport=` needs no consumer: all 15
/// stock occurrences are commented out. The eight-facing report selection is
/// `Report[facing_u16 % Report.Count]` (`0x006FF349`..`0x006FF393`), and no
/// stock weapon authors a comma-separated `Report=` — 0 of 223 — so a single
/// report is observationally identical. Two of the three voice guards native
/// carries are already here: re-selecting an already-selected object emits
/// nothing (`ObjectClass::Select @ 0x005F4520` returns false), and the
/// one-voice-per-batch latch matches `g_SelectionVoice_Enable @ 0x00822CF2`.
///
/// **The repeat guard is landed.** It is not a timer: each techno owns a
/// pending-voice index (`+0x4F0`, sentinel -1), a live handle (`+0x4DC`) and
/// the playing index (`+0x4F4`); `TechnoClass::Queue_Voice @ 0x00708D90` only
/// latches and `TechnoClass::AI_Update @ 0x006F9EBB` drains — handle free
/// plays, handle live with the SAME index drops, handle live with a different
/// index holds and retries next pass. Voices are non-positional (volume 1.0f,
/// pan 0x2000, `0x006F9EE0`/`0x006F9EE5`). The layering worry turned out not to
/// bind: VERA emits acknowledgement lines from the **app** input layer, never
/// from `sim/`, so the whole guard lives in [`crate::audio::voice_queue`] as a
/// device-free decision module and `sim/` never learns about audio.
///
/// **Still absent on the voice side.** `VoiceDeploy=`/`VoiceUndeploy=`
/// (deploy/unload orders), `VoiceCrashing=` (7) and
/// `VoiceSinking=`/`VoiceFalling=` are not parsed, and taunts reach an empty
/// match arm. The native consumers of those slots were not isolated, so no
/// mapping is asserted.
/// - Player effect: those orders and states stay silent.
/// - Frequency: deploy orders are common; crashing/sinking lines need an
///   aircraft or ship kill.
///
/// `VoiceFeedback=` (133 stock authors) is **no longer among them** — see
/// [`Self::VoiceFeedback`].
///
/// **Partly closed.** `MoveSound=` now has both halves: the sim's
/// post-locomotor tail (`Simulation::tick_move_sound_after_process`) emits
/// [`Self::AnimationStarted`]/[`Self::AnimationStopped`] keyed on the object,
/// and `audio::arbiter` gives that key a real `VocHandle` — so a
/// `Control=loop` entry such as `RocketeerMoveLoop` sustains for the whole
/// move and a `Control= random predelay` entry such as
/// `GrizzlyTankMoveStart` plays one start-up sample, which is what gamemd
/// does.
///
/// **Also landed.** `VoiceCapture=` is routed (`0x00708DC0` reads
/// `TechnoTypeClass+0x55C` and only falls through to the Enter slot at
/// `0x00709020` when it is absent), so an engineer capture speaks the capture
/// line instead of the move line. `[AudioVisual] BuildingDieSound=` is emitted
/// when a dying building's type carries no `DieSound=` of its own
/// (`BuildingClass::DestructionEffects`, `0x0044173F`..`0x00441779`),
/// `[AudioVisual] LightningSounds=` is played at each bolt strike
/// (`LightningStorm::GroundStrike @ 0x0053A45F..0x0053A4A2`), and
/// `[AudioVisual] BaseUnderAttackSound=` now rides with the under-attack EVA
/// line (`HouseClass::NotifyUnderAttack @ 0x004F95B8..0x004F95CF`), on the
/// building branch only.
///
/// `[AudioVisual] BuildingDamageSound=` (`Rules+0x714`) is landed too. It is a
/// **damage-state** cue, not a per-hit one: `BuildingClass::ReceiveDamage @
/// 0x00442230` only enters the arm through
/// `0x00442476 JMP [EAX*4 + 0x00442C18]` with `EAX = result - 2`, so results 2
/// (the hit crossed HP from `>= Strength >> 1` to below it) and 3 (it crossed
/// below `Strength * Rules+0x1708`, ConditionRed) reach
/// `0x004426D2 CMP [type+0x538],-1` while an ordinary non-crossing hit
/// (result 1) plays nothing. A dead building is skipped earlier still by
/// `0x0044242C MOV AL,[ESI+0x90]` (`ObjectClass::IsAlive`). Frequency: twice
/// per building on the way down in a sustained base attack, plus every
/// repair-and-re-damage cycle across a threshold — not the every-shot cadence
/// an earlier pass assumed. Stock `BuildingDamageSound=BuildingDamaged` is a
/// five-sample `[SoundList]`, and only 34 of all stock types author a
/// `DamageSound=` of their own (30 `BuildingMetalDamaged`, 4 `Dummy`), so the
/// global covers nearly every structure.
///
/// `[AudioVisual]`'s companion damage voice is landed as well — see
/// [`GameSoundEvent::VoiceFeedback`] for `TechnoClass::ReceiveDamage @
/// 0x00701900`'s result-2 arm at `0x00702695`.
///
/// **Still absent on the damage side: the per-type `DamageSound=` cue.**
/// `TechnoTypeClass+0x538` is parsed but only *read as a gate*. Native plays
/// it from arm `0x00702713`/`0x00702717` of the same switch — **index 1 of the
/// table at `0x00702D24`, the ordinary NON-crossing hit** (damage result 1),
/// not the crossings the global rides. So the per-type and the global cue are
/// mutually exclusive by *result*, not merely by the `-1` gate: a type with
/// its own `DamageSound=` gets a clang on every ordinary hit and **nothing**
/// on a crossing, while a type without one is silent on ordinary hits and
/// sounds the global on crossings. The arm also plays the sound **twice** —
/// two identical blocks at `0x00702760` and `0x007027A9`, each re-reading
/// `[type+0x538]` and the same coordinate `[ESI+0x9C]`.
/// Trigger: any non-crossing hit on a type that authors the key. Player
/// effect: those types are silent under ordinary fire where native double-taps
/// a clang. Frequency: all 34 stock authors are civilian/neutral props (30
/// `CA*` structures plus `AMMOCRAT` on `BuildingMetalDamaged`, 4 `CAARMY0*` on
/// `Dummy`) — no player-built structure and no unit authors it, so this is
/// "whenever you shoot civilian scenery", common on urban maps and absent from
/// a pure base fight. Downstream risk: landing it needs a distinct
/// `DamageState::Damaged` (result 1) emission, which VERA classifies but does
/// not currently route.
///
/// **Also unrecorded until now: an unresolvable per-type `DamageSound=`.**
/// `TechnoTypeClass+0x538` is `-1` both when the key is absent *and* when the
/// name resolves to no Voc, so native falls back to the global in both cases;
/// VERA's `damage_sound: Option<String>` is `Some` for any non-empty string
/// and suppresses the global. Trigger: a type naming a sound that is not in
/// `soundmd.ini`. Player effect: that structure is silent where native plays
/// the global cue. Frequency: never on retail — `BuildingMetalDamaged` and
/// `Dummy` both register. Downstream risk: none; closing it means resolving
/// the name against the sound registry, which lives above `rules/`.
///
/// **Still absent.** Nothing emits the other ambient families:
/// `WorkingSound=` (9 stock), `AuxSound1=` (8), `AuxSound2=` (5),
/// `TurretRotateSound=` (2) and the water enter/leave pair have no producer.
/// - Player effect: the soundscape is still thin — bases have no working hum.
/// - Frequency: continuous.
/// - Downstream risk: none left on the audio side. Each remaining family needs
///   only a sim-side producer and the same owner-keyed handle `MoveSound=`
///   already uses; the arbiter behind it is landed.
///
/// **`[AudioVisual]` keys with a verified native consumer but no VERA producer
/// yet**, all read by `RulesClass::ReadAudioVisual @ 0x006691E0` and all
/// UNCHECKED against their own callers unless noted:
/// - `EnterGrinderSound`/`LeaveGrinderSound` (`+0x268`/`+0x26C`),
///   `EnterBioReactorSound`/`LeaveBioReactorSound` (`+0x270`/`+0x274`): no
///   grinder or bio-reactor consumer exists in `sim/` at all.
/// - The seven `Crate*Sound` keys (`+0x1E4`..`+0x1FC`): `sim/crates` places
///   and regenerates crates but nothing picks one up, so there is no VERA
///   producer to hang them on. The gap is a crate-pickup gap, not an audio one.
/// - `MindClearedSound` (`+0x264`) — native consumer
///   `CaptureManagerClass::FreeUnit @ 0x004720C5`; `MasterMindOverloadDeath`‑
///   `Sound` (`+0x258`) — `CaptureManagerClass::Update @ 0x00471B39`;
///   `PlaceBeaconSound` (`+0x1CC`) — `RadarClass::PlaceBeacon @ 0x00430D8E`
///   (each found by an operand sweep for that `RulesClass` offset, which is
///   capped and therefore not an exhaustive enumeration of readers). VERA has
///   no mind-control release path, no MasterMind overload and no beacons, so
///   none of the three has a producer to wire.
/// - `ImpactWaterSound` (`+0x200`): no producer; the native reader was not
///   isolated. (`CreditTicks` (`+0x6D0`, count `+0x6DC`) is landed as
///   [`GameSoundEvent::CreditTick`] — reader `CreditsClass::Draw @ 0x004A24F4`.)
/// - **Dead on retail data, so not parsed:** `ImpactLandSound` (`+0x204`) and
///   `IceCrackSounds` (`+0x644`) are both **empty** in stock `rulesmd.ini`;
///   `GateUp`/`GateDown` (`+0x404`/`+0x408`) and `Construction` (`+0x6C8`) are
///   `Dummy`; `CreateUnitSound`/`CreateInfantrySound`/`CreateAircraftSound`
///   (`+0x178`..`+0x180`) and `LeaveGrinderSound` are empty. `Dummy` is one of
///   the eight `[SoundList]` ids with no usable `Sounds=`, so it registers
///   silently. Parsing any of these would add a field with no player effect.
///
/// **Superweapon launch cues are landed** — see
/// [`GameSoundEvent::SuperWeaponActivated`] and
/// [`crate::app::match_runtime::sound_dispatch::superweapon_launch_cue`]. Two
/// residuals remain there: the EVA suppression flag `[0x00A8B538]` has no VERA
/// equivalent (VERA has no defeated-spectator state), and the `MultiMissile`,
/// `ChronoSphere`, `ChronoWarp`, `PsychicDominator` and `SpyPlane` cases have
/// no sim launch handler yet, so their rows in the table are mapped but never
/// reached.
#[derive(Debug, Default)]
pub struct SoundEventQueue {
    events: Vec<GameSoundEvent>,
}

impl SoundEventQueue {
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Push a sound event into the queue.
    pub fn push(&mut self, event: GameSoundEvent) {
        self.events.push(event);
    }

    /// Drain all pending events for playback.
    pub fn drain(&mut self) -> Vec<GameSoundEvent> {
        std::mem::take(&mut self.events)
    }

    /// Whether there are pending events.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sound_id_accessor() {
        let evt: GameSoundEvent = GameSoundEvent::WeaponFired {
            sound_id: "VGCannon1".to_string(),
            source: None,
        };
        assert_eq!(evt.sound_id(), "VGCannon1");
    }

    /// An EVA event carries the `[DialogList]` name and the call-site type
    /// override, never a resolved sample; it is non-positional.
    #[test]
    fn eva_event_carries_the_event_name_and_type_override() {
        let evt: GameSoundEvent = GameSoundEvent::Eva {
            event: "EVA_StructureGarrisoned".to_string(),
            type_override: None,
        };
        assert_eq!(evt.sound_id(), "EVA_StructureGarrisoned");
        assert_eq!(evt.screen_pos(), None);
        assert_eq!(evt.source(), None);

        let evt = GameSoundEvent::Eva {
            event: "EVA_BattleControlTerminated".to_string(),
            type_override: Some(crate::rules::sound_ini::EvaType::Interrupt),
        };
        match evt {
            GameSoundEvent::Eva { type_override, .. } => {
                assert_eq!(
                    type_override,
                    Some(crate::rules::sound_ini::EvaType::Interrupt)
                );
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_building_garrisoned_sfx_screen_pos_accessor() {
        let evt: GameSoundEvent = GameSoundEvent::BuildingGarrisonedSfx {
            sound_id: "BuildingGarrisoned".to_string(),
            source: Some(SoundSource::new((100.0, 200.0), (3, 4))),
        };
        assert_eq!(evt.sound_id(), "BuildingGarrisoned");
        assert_eq!(evt.screen_pos(), Some((100.0, 200.0)));
        assert_eq!(evt.source().map(|s| s.cell()), Some((3, 4)));
    }

    #[test]
    fn test_chute_sound_screen_pos_accessor() {
        let evt = GameSoundEvent::ChuteSound {
            sound_id: "ParachuteDrop".to_string(),
            source: Some(SoundSource::new((128.0, 256.0), (5, 6))),
        };
        assert_eq!(evt.sound_id(), "ParachuteDrop");
        assert_eq!(evt.screen_pos(), Some((128.0, 256.0)));
    }

    #[test]
    fn test_bridge_repaired_carries_spatial_and_eva_sound_ids() {
        let evt = GameSoundEvent::BridgeRepaired {
            sound_id: "BridgeRepaired".to_string(),
            source: Some(SoundSource::new((32.0, 64.0), (1, 2))),
            eva_event: Some("EVA_BridgeRepaired".to_string()),
        };

        assert_eq!(evt.sound_id(), "BridgeRepaired");
        assert_eq!(evt.screen_pos(), Some((32.0, 64.0)));
        match evt {
            GameSoundEvent::BridgeRepaired { eva_event, .. } => {
                assert_eq!(eva_event.as_deref(), Some("EVA_BridgeRepaired"));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_queue_drain() {
        let mut queue: SoundEventQueue = SoundEventQueue::new();
        assert!(queue.is_empty());
        queue.push(GameSoundEvent::UiSound {
            sound_id: "click".to_string(),
        });
        queue.push(GameSoundEvent::UiSound {
            sound_id: "beep".to_string(),
        });
        assert!(!queue.is_empty());
        let events: Vec<GameSoundEvent> = queue.drain();
        assert_eq!(events.len(), 2);
        assert!(queue.is_empty());
    }
}
