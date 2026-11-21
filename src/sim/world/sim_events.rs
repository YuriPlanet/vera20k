//! Simulation output events: the sound, EVA and fire records a frame emits
//! for the app layer to drain, with their constructors.

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::SoundName;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::combat::combat_weapon::WeaponSlot;
use crate::sim::components::DriveCoord;
use crate::sim::components::Position;
use crate::sim::intern::InternedId;
use crate::sim::radar::RadarEventRequest;
use crate::util::fixed_math::SimFixed;

/// The alliance voices MakeAlly and MakeEnemy queue for the local House.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllianceEva {
    /// `EVA_AllianceFormed` (`0x008247E4`): the local House is one of the
    /// pair or allied with both.
    Formed,
    /// `EVA_EnemyAllianceFormed` (`0x008247CC`): any other client.
    EnemyFormed,
    /// `EVA_RequestingAlliance` (`0x008247B4`): the local House asked.
    Requesting,
    /// `EVA_AllianceRequested` (`0x0082479C`): the local House was asked.
    Requested,
    /// `EVA_AllianceBroken` (`0x00824808`).
    Broken,
}

impl AllianceEva {
    pub fn event(self) -> &'static str {
        match self {
            Self::Formed => "EVA_AllianceFormed",
            Self::EnemyFormed => "EVA_EnemyAllianceFormed",
            Self::Requesting => "EVA_RequestingAlliance",
            Self::Requested => "EVA_AllianceRequested",
            Self::Broken => "EVA_AllianceBroken",
        }
    }
}

/// The lines Ares 3.0 `10013E40` gives a captured tech building in place of
/// `0x0044840B..0x0044848F`: the old owner's player hears the type's
/// `LostEvaEvent` (default `EVA_TechBuildingLost`; `None` is silent) and
/// reads `Message.Lost`, the new owner's player hears `CaptureEvaEvent=`
/// and reads `Message.Capture` (`None`: an empty label prints nothing).
/// A game without Ares carries the default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AresTechCaptureLines {
    pub lost_eva_event: Option<InternedId>,
    pub message_lost: Option<InternedId>,
    pub message_capture: Option<InternedId>,
}

/// What Ares FootClass_Destroyed (10049080, hook 0x4D98C0) and
/// BuildingClass_Destroyed_UnitLost (10013F10, hook 0x44D760) add to
/// [`SimSoundEvent::UnitLost`]. A game without Ares carries none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AresUnitLostLine {
    /// The victim type's `EVA.Lost`, spoken whether or not the event's
    /// `radar` is admitted.
    pub eva: InternedId,
    /// A Foot creates the radar event; a building creates none.
    pub radar_event: bool,
}

/// The native playback destination for a map-authored sound reference.
#[derive(Debug, Clone)]
pub enum TriggerSoundTarget {
    Centered,
    Eva,
    Coordinate {
        coordinate: DriveCoord,
        flags: u32,
    },
    Object {
        owner: u64,
        world: crate::sim::anim_class::AnimWorldCoord,
    },
}

/// A sound event produced during simulation (combat, death, production).
/// Pure data — no audio library dependency. Drained by the app layer each frame.
///
/// A sound named by rules travels as a [`SoundName`] of the rules' own
/// sound-name table ([`crate::rules::ruleset::SoundNames`]), never as an id
/// of the gameplay interner: interned ids are saved and hashed, and a
/// Main-selected cue interned on first use would move them. Neither these
/// events nor their channel are saved or hashed.
#[derive(Debug, Clone)]
pub enum SimSoundEvent {
    TriggerAudio {
        reference: crate::rules::sound_ini::SoundReference,
        target: TriggerSoundTarget,
    },
    /// PlanMgr638593 accepted an editing order; audio gates the local issuer.
    PlanningNodeAdded { owner: InternedId },
    /// TAction21 Execute6DD8B0 calls VoxQueueVoice752480 with its literal
    /// signed DialogList index, type2 and priority-1. The process audio
    /// catalog owns ordinal resolution; no cue-name guess enters simulation.
    TriggerEva { index: i32 },
    /// A pending acknowledgement owner's live AI reached Techno6F9EBB.
    /// The app owns the latch/handle; this fact preserves the actual visit.
    UnitVoiceVisit { owner: u64 },
    /// An admitted QueueVoice708D90 request, before any later positional
    /// cue. The sole audio owner latches it; only a reached Techno head drains.
    UnitVoiceRequested { owner: u64, sound_id: SoundName },
    /// Techno destructor6F4607 destroys its voice handle before its owned
    /// Anims. The app discards both pending and playing voice identities.
    UnitVoiceDestroyed { owner: u64 },
    /// A deploy conversion copied the voice handle, pending and playing slots
    /// (`+0x4DC..+0x4F4`) to the object replacing `from` and emptied `from`'s:
    /// UnitClass::Deploy739971, BuildingClass::Mission_Selling44A0D4.
    UnitVoiceMoved { from: u64, to: u64 },
    /// Voc7509E0 on the object's handle (Building+6A0 for Construction).
    /// The existing app/audio handle owner performs looping updates and release.
    ObjectSoundStarted {
        owner: u64,
        sound_id: InternedId,
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// Start/report sound on an object or animation handle.
    AnimationStarted {
        anim_id: crate::sim::anim_class::AnimId,
        sound_id: SoundName,
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// Hard-stop the owner's handle, then optionally play StopSound. Anim
    /// destruction emits ObjectSoundReleased first so its Report plays out.
    AnimationStopped {
        anim_id: crate::sim::anim_class::AnimId,
        stop_sound_id: Option<InternedId>,
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// `IncreaseGattlingStage`'s report (`VocClass::PlayAt @ 0x0070DFDD` on
    /// the techno's `+0x4A4` handle): start the stage weapon's loop, stopping
    /// any loop the handle held. `owner` is `combat::gattling::
    /// gattling_sound_owner(techno)`.
    GattlingLoop {
        owner: u64,
        sound_id: SoundName,
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// A stage-up's hard stop of that loop (`VocHandle::StopAndClear @
    /// 0x00405D40`).
    GattlingLoopStop { owner: u64 },
    /// A decay's or Limbo's release of that loop (`SoundEvent::Release @
    /// 0x00406060`): it stops repeating and plays out.
    GattlingLoopRelease { owner: u64 },
    /// `SoundEvent::Release @ 0x00406060` on an object's own sound handle
    /// (`FootClass+0x544` or `AnimClass+0x1A0`, keyed by object id). A one-shot
    /// plays out; an uncounted loop stops repeating. Anim UnInit4255D5 and
    /// scalar destructor4228E0 share this operation with Foot4D3677.
    ObjectSoundReleased { owner: u64 },
    /// FootLimbo4DB353 ->405FD0: stop repetition for any valid event, even
    /// a counted loop, and detach the handle. The current audio plays out.
    ObjectSoundDetached { owner: u64 },
    /// Native Fly AuxSound1/AuxSound2 at the phase callback world coordinate.
    AircraftPhase {
        sound_id: InternedId,
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// An entity was destroyed — play its VoiceDie=/DieSound= at its
    /// Location (`0x0070213F..0x0070218B`, `0x007021A9..0x007021FB` copy
    /// `[obj+0x9C..0xA4]` for `VocClass::PlayAt 0x007509E0`).
    EntityDied {
        die_sound_id: SoundName,
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// An entity was crushed by a vehicle — play its CrushSound= (the squish).
    /// Normal crush teardown does not also enter the ordinary DieSound path.
    EntityCrushed {
        crush_sound_id: InternedId,
        /// The CRUSHER's Location, read before the victim dies:
        /// `UnitClass::Crush_Or_Prescatter_Cell @ 0x007416A0`
        /// (`0x007418AA..0x007418DC`, `[EDI+0x9C..0xA4]`) and
        /// `JumpjetLocomotionClass::State5_Crash @ 0x0054CA90`
        /// (`0x0054CCDE..0x0054CD14`).
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// DeploySound on infantry stance entry, successful unit-to-building
    /// conversion, or a building's undeploy start (Selling's UndeploysInto
    /// arm plays the building type's own `DeploySound=`).
    EntityDeployed {
        deploy_sound_id: InternedId,
        /// The deploying object's Location: Infantry Do_Action
        /// (`0x0051D94F..0x0051D9CA`), `UpdateSimpleDeployState @ 0x00739AC0`
        /// (`0x00739C86..0x00739CB9`), `UnitClass::Deploy @ 0x007393C0`
        /// (`0x00739A21..0x00739A54`, the MCV's) and `Mission_Selling @
        /// 0x00449C30` (`0x0044A9F9..0x0044AA38`, the building's north-west
        /// cell centre, not its foundation centre).
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// An infantry entity entered the Undeploying phase — play its UndeploySound=.
    EntityUndeployed {
        undeploy_sound_id: InternedId,
        /// The object's own Location: Infantry Do_Action
        /// (`0x0051D998..0x0051D9CA`), `UnitClass::Undeploy @ 0x00739CD0`
        /// (`0x00739E81..0x00739EB3`).
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// A building finished construction — play EVA "Construction complete".
    /// `type_id` is the factory's already-resolved completed product; the app
    /// names it in the optional accessibility text line. Transient event only:
    /// drained each frame, never a gameplay hash or persisted identity input.
    BuildingComplete {
        owner: InternedId,
        type_id: InternedId,
    },
    /// `HouseClass::Place_Production 0x004FB5C6..0x004FB644`: a unit left the
    /// factory of a human-controlled house. `EVA_UnitReady` plays for the
    /// local owner once its client admits `radar` (type 6). `type_id` is the
    /// held product the factory completed, carried for the optional text line.
    UnitComplete {
        owner: InternedId,
        radar: RadarEventRequest,
        type_id: InternedId,
    },
    /// One accepted HouseClass win/loss transition. The app resolves the
    /// local owner's faction-specific STANDARD EVA; this edge is transient so
    /// loading a mid-Savour snapshot cannot replay the announcement.
    MatchOutcome {
        owner: InternedId,
        kind: crate::sim::house_state::HouseOutcomeKind,
    },
    /// Global `[AudioVisual] SellSound=` for a successful wall-sale event.
    /// The EventClass receiver can differ from the wall owner. A unit's sale
    /// (`FootClass::On_Sold @ 0x004D9F70`) plays the same cue for its owner.
    WallSold { receiver: InternedId },
    /// A deploy command failed target placement validation.
    /// App layer gates this to the local human player and plays
    /// `EVA_CannotDeployHere`.
    CannotDeployHere { owner: InternedId },
    /// `FactoryClass::StartProduction` refused to queue a build
    /// (`0x004C9D3B..0x004C9D5F`): `[AudioVisual] ScoldSound=` when the house
    /// is the local player's.
    ProductionRefused { owner: InternedId },
    /// The PLACE event's Unlimbo succeeded (`HouseClass @ 0x004FB0E0`,
    /// `0x004FB236`; a wall's Unlimbo also returns 1 at `0x00440865`):
    /// `[AudioVisual] BuildingSlam=` centred at full volume when the house is
    /// the local player's (`0x004FB2CC..0x004FB314`). `building_type` is the
    /// placed building's type, whose Ares `SlamSound` replaces the cue.
    BuildingPlaced {
        owner: InternedId,
        building_type: InternedId,
    },
    /// A chrono teleport happened — play the resolved warp sound at this position.
    /// Sim emits two of these per warp: one at the departure Location with the
    /// unit's `ChronoOutSound=` (`0x0071960A..0x0071962C`, Chronosphere state
    /// 2 `0x007198CE`), one at the arrival Location, after SetHeight(0) and
    /// Mark(DOWN), with its `ChronoInSound=` (`0x007196EE..0x00719710`, state
    /// 5 `0x00719A70`).
    ChronoTeleport {
        sound_id: InternedId,
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// `TechnoClass::StartUncloaking @ 0x007036C0` accepted an arg-zero
    /// state-1/2 transition and called positional `VocClass::PlayAt @
    /// 0x007509E0` with `[AudioVisual] CloakSound` at the Techno world coord.
    CloakSound {
        sound_id: SoundName,
        rx: u16,
        ry: u16,
        sub_x: SimFixed,
        sub_y: SimFixed,
        world_z_leptons: i32,
    },
    /// `UnitClass::PerCellProcess @ 0x0073B036..B04D`: a crusher flattened the
    /// overlay under it and played the overlay type's `CrushSound=`
    /// (`OverlayTypeClass+0x1F0`) positionally at its own coordinates.
    WallCrushed {
        sound_id: SoundName,
        rx: u16,
        ry: u16,
        sub_x: SimFixed,
        sub_y: SimFixed,
        world_z_leptons: i32,
    },
    /// `VocClass::PlayAtPos @ 0x00750920` of a rules-named sound, centred
    /// (`0x2000`), full volume (`1.0f`), without an owner handle. Walk's
    /// exhausted-retry branch 0x0075B085..0x0075B0B0 requests ScoldSound this
    /// way. Evidence: `tools/spatial_oracle/foot_scold_latch.{py,json,md}`.
    VocCentered { sound_id: SoundName },
    /// The same centred `0x00750920` call with a raw SOUNDMD ordinal: team
    /// script action 25 (`0x006E9901`). The process catalog resolves it;
    /// -1 or an unknown ordinal plays nothing (`0x0075093B`).
    VocCenteredIndex { index: i32 },
    /// `VocClass::PlayAt @ 0x007509E0` of a rules-named sound at an object's
    /// Location: the mind-control capture, release and overload sounds.
    /// `audible_to` names the houses a `HouseClass::IsHumanPlayer
    /// @ 0x0050B6F0` gate admits (the local player must own one of them; the
    /// app resolves it); `None` plays for everyone.
    VocAt {
        sound_id: SoundName,
        audible_to: Option<[InternedId; 2]>,
        rx: u16,
        ry: u16,
        sub_x: SimFixed,
        sub_y: SimFixed,
        world_z_leptons: i32,
    },
    /// VocClass::PlayAtCoord750E20 retains the full signed native coordinate
    /// and owns its playback handle independently of the source object.
    VocAtCoord {
        sound_id: SoundName,
        coordinate: DriveCoord,
        flags: u32,
    },
    /// VocClass750F40 detaches independent coordinate requests whose flags
    /// overlap and whose full signed XYZ equals the supplied coordinate.
    VocStopAtCoord { coordinate: DriveCoord, flags: u32 },
    /// `TechnoClass::AI_Update @ 0x006FA054..0x006FA145` crossed a rank.
    ///
    /// Native plays `[AudioVisual] UpgradeVeteranSound=`/`UpgradeEliteSound=`
    /// positionally at the object (`VocClass::PlayAt @ 0x007509E0`) and the
    /// `EVA_UnitPromoted` voice (`0x00752700`), both only when
    /// `HouseClass::IsHumanPlayer @ 0x0050B6F0` holds for the owner — in a
    /// skirmish that is `owner == the local player`, which the app resolves.
    /// `sound_id` is `None` when the rules key is absent or empty (native's
    /// invalid Voc index is silence).
    /// `type_id` is the promoted object's already-resolved type reference; the
    /// app names it in the optional accessibility text line. Transient event
    /// only: drained each frame, never a gameplay hash or persisted identity.
    UnitPromoted {
        owner: InternedId,
        sound_id: Option<InternedId>,
        type_id: InternedId,
        /// The object's Location (`0x006FA092..0x006FA0BC`,
        /// `0x006FA0FF..0x006FA12A`); with Ares, its Transporter's when it
        /// rides in one (UpdateVeterancy10046D2F..10046D40).
        world: crate::sim::anim_class::AnimWorldCoord,
        /// Ares `EVA.VeteranPromoted`/`EVA.ElitePromoted`
        /// (VoxClass::PlayIndex at10046D54); `None` is the silent -1 index.
        /// Read only when Ares plays; the stock event carries `None`.
        eva: Option<InternedId>,
    },
    /// A building / harvester admitted a damage or first Temporal-head warning.
    /// For the local owner the app admits `radar` (type 3 base / type 4 miner) on its client
    /// event array; that result is native's only rate limit on the EVA voice.
    UnderAttack {
        rx: u16,
        ry: u16,
        owner: InternedId,
        miner: bool,
        radar: RadarEventRequest,
    },
    /// `HouseClass::NotifyUnderAttack @ 0x004F93E0`, non-local branch
    /// (`0x004F955D..0x004F95B3`): a building of a house that lists `owner`
    /// (a human house) as its ally admitted a warning. Once the listener's
    /// client admits `radar` (`CreateRadarEvent(0x10, cell)`):
    /// `EVA_OurAllyIsUnderAttack` plus the `BaseUnderAttackSound` siren, both
    /// for `owner`.
    AllyUnderAttack {
        owner: InternedId,
        radar: RadarEventRequest,
    },
    /// `TechnoClass::Death_Announcement @ 0x004D98C0` fired for a unit of
    /// `owner` (a human house) that is not `Spawned=`. The app plays
    /// `EVA_UnitLost` for the local owner once its client admits `radar`
    /// (`CreateRadarEvent(7, cell)`, 8-cell dedupe). `type_id` is the dead
    /// techno's own interned type, resolved at the death site; transient.
    UnitLost {
        owner: InternedId,
        radar: RadarEventRequest,
        type_id: InternedId,
        /// The Ares announcement, when Ares plays: the app then speaks its
        /// line instead, whatever `radar` answers. `None` is the stock event.
        ares: Option<AresUnitLostLine>,
    },
    /// A `PlayEVA` line native speaks only for the player's house: the
    /// `HouseClass::Update` advice (`EVA_InsufficientFunds` `0x004F8BA0`,
    /// `EVA_LowPower` `0x004F8D14`), the depot's empty wallet, the crate
    /// upgrades and the Robot Tank lines (`sim::powered_unit`). `event` is
    /// the `evamd.ini` section name; the app applies the local-owner test.
    HouseEva {
        owner: InternedId,
        event: &'static str,
    },
    /// `BuildingClass::Mission_Selling @ 0x00449C30`'s completing stage-2 visit
    /// (`0x00449CC1..0x00449CE5`, once the pack-up animation has set
    /// `+0x6DD`): `TechnoClass+0x41A` (owner is the local player) and no
    /// `UndeploysInto=` (`Type+0x408`) — an undeploying building stays
    /// silent. Each removed upgrade uses the same event at
    /// `0x0044AB22..0x0044AB36`. App plays `EVA_StructureSold`. `type_id`
    /// names the building that was sold (or whose upgrade was removed); the
    /// emit site already resolved it. Transient accessibility fact only.
    StructureSold {
        owner: InternedId,
        type_id: InternedId,
    },
    /// A sale order's click (`BuildingClass::Sell_Back @ 0x00447110`:
    /// `VocClass::PlayAtPos` of `[AudioVisual] GenericClick=`) for the
    /// owner's player (`HouseClass::IsHumanPlayer @ 0x0050B6F0`; the app
    /// resolves it).
    SellClick { owner: InternedId },
    /// `BuildingClass::ToggleRepair @ 0x00446FF0` (`0x004470B7`): repair
    /// switched on while `Health != Type.Strength` (`0x00447059`) and the
    /// owner is the local player (`0x004470A4 CALL 0x0050B6F0`). App plays
    /// `EVA_Repairing` for the local owner. Depot MissionRepair44C507 emits
    /// the same request when its first paid service step starts. `type_id` is
    /// the building being repaired (ToggleRepair) or the depot that began the
    /// service (MissionRepair); both are already resolved at the emit site.
    /// Transient accessibility fact only.
    Repairing {
        owner: InternedId,
        type_id: InternedId,
    },
    /// MissionRepair reaches CreateRadarEvent8 after a service completes.
    /// Hospital44B93F already admits House50B6F0; depot44BD96 leaves the
    /// local-owner+41A gate to the app. Both use its existing radar admission
    /// before EVA_UnitRepaired.
    UnitRepaired {
        owner: InternedId,
        owner_admitted: bool,
        radar: RadarEventRequest,
    },
    /// `BuildingClass::ChangeOwner @ 0x00448260` announce block
    /// (`0x004483C0..0x0044848F`) for an engineer capture
    /// (`InfantryClass::PerCellProcess 0x00519F79 PUSH 1` = announce). Native
    /// gate: the OLD or the NEW owner is the local player (`0x004483C6` /
    /// `0x004483D1 CALL 0x0050B6F0`), the new owner's type not
    /// `MultiplayPassive` (`0x004483E1`). `tech_building` is `NeedsEngineer=`
    /// (`Type+0x1552`, `0x00448401`): clear → `CreateRadarEvent(10, cell)`
    /// (`0x00448472 MOV ECX,0xA`) and, when accepted, `EVA_BuildingCaptured`
    /// (`0x0044848A`); set → `EVA_TechBuildingLost` for a local OLD owner
    /// (`0x00448415`, ECX = `this->Owner`) and the type's `CaptureEvaEvent=`
    /// (`Type+0x1554`, `0x00448459 QueueVoice`) for a local NEW owner. The
    /// sim cannot see the local player: it emits for any human-controlled
    /// side and the app applies the local test. `radar` is the type-10
    /// request of an ordinary building; a tech building reaches none.
    BuildingCaptured {
        old_owner: InternedId,
        new_owner: InternedId,
        tech_building: bool,
        radar: Option<RadarEventRequest>,
        capture_eva_event: Option<InternedId>,
        /// Ares 3.0 `BuildingClass_ChangeOwnership_Tech @ 10013E40` (hook
        /// `0x0044840B`) replaces the tech building's block with these when
        /// Ares plays; otherwise the default.
        ares_tech: AresTechCaptureLines,
    },
    /// `BuildingClass::Infiltrate @ 0x004571E0`'s announcements
    /// (`sim::spy_infiltration`). When the building's owner or the spy's
    /// house is the local player (`0x004571FE`, `0x0045720E`, both
    /// `0x0050B6F0`), `CreateRadarEvent(9, cell)` runs (`0x0045722F`), and
    /// its answer gates most of the lines `effect` picks
    /// (`eva_producers::infiltration_eva_events`). The sim cannot see the
    /// local player: it emits when either side is human-controlled and the
    /// app applies the local test.
    BuildingInfiltrated {
        owner: InternedId,
        infiltrator: InternedId,
        effect: crate::sim::spy_infiltration::InfiltrationEffect,
        radar: RadarEventRequest,
    },
    /// `SuperClass::AI_Ready @ 0x006CBCA0` (`0x006CBE63`): the charge
    /// expired and `+0x6F` (IsReady) was set (`0x006CBDCC`); the announce
    /// argument is `house == PlayerPtr` (`0x004F8E42..0x004F8E47` in
    /// `HouseClass::Update`). `sw_type` is the `[SuperWeaponTypes]` section;
    /// the app maps its `Type=` onto the `*Ready` line through the
    /// `0x006CBDE6` jump table (`0x006CBEA8`).
    SuperWeaponReady {
        owner: InternedId,
        sw_type: InternedId,
    },
    /// `HouseClass @ 0x0050AF10` (label `HouseClass__Update_Owned_Supers`):
    /// a granted Super's hold changed (`SuperClass::Suspend @ 0x006CB4D0`
    /// returned true) or it was lost (`SuperClass @ 0x006CB7B0` returned
    /// true). For the player's house native then clears the selected Super
    /// (`0x008809A0`) when it is this one (`0x0050B181..0x0050B190`); the app
    /// applies that local-owner half.
    SuperWeaponStatusChanged {
        owner: InternedId,
        sw_type: InternedId,
    },
    /// `BuildingClass::OnConstructionComplete @ 0x00445F80`
    /// (`0x004468AD..0x00446995`): a building whose type carries
    /// `SuperWeapon=` finished building up. Native speaks only when the owner
    /// is not the local player (`0x004468B3`), not allied with it
    /// (`0x004468CD IsAlliedWith(PlayerPtr)`), `[0xA8B538] == 0` (local
    /// player not defeated, `0x004468DA`), `GameMode != 0` (`0x004468E7`),
    /// and the type's `AuxBuilding=` is absent or owned by the building's
    /// owner (`0x0044692E CountOwnedInstances`); the line comes from the
    /// `[SuperWeaponTypes]` list index (`0x00446948` table at `0x00446FC0`).
    /// The app applies the listener gates and the table.
    SuperWeaponDetected {
        owner: InternedId,
        sw_type: InternedId,
    },
    /// `HouseClass::MPlayer_Defeated @ 0x004FC0B0` defeated a non-observer
    /// House (`0x004FC26F` local, `0x004FC34D` remote; a remote
    /// `MultiplayPassive` House never gets here, `0x004FC317`). The app posts
    /// `TXT_PLAYER_DEFEATED` with the House's UI name and plays
    /// `EVA_YouHaveLost` for the local House, `EVA_PlayerDefeated` otherwise.
    PlayerDefeated { house: InternedId },
    /// MakeAlly or MakeEnemy changed `owner`'s alliance with `other` outside
    /// initialization (`sim::house_diplomacy`). With `line`, the app posts
    /// `TXT_HAS_ALLIED` / `TXT_AT_WAR` (owner, other) in the owner's
    /// HouseType colour; `eva` is the local client's voice, if any.
    AllianceChanged {
        owner: InternedId,
        other: InternedId,
        formed: bool,
        line: bool,
        eva: Option<AllianceEva>,
    },
    /// ForceShield Launch6CD176 plays StartSound before its blackout/curtain
    /// receivers. The later SuperWeaponLaunched owns only that case's local
    /// selection/Ready-line cleanup; the original signed deck is retained.
    SuperWeaponActivation {
        sw_type: InternedId,
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// A superweapon fired. `sw_type` is the interned `[SuperWeaponTypes]`
    /// section name of the launched weapon — the discriminator the app layer
    /// needs to pick the cue, and the same object gamemd switches on.
    ///
    /// `SuperClass::Launch @ 0x006CC390` switches on `*(param_1[10] + 0xB4)`,
    /// the launched `SuperWeaponTypeClass`'s `Type=` index, and each case
    /// plays its own cue: nothing at all for `ChronoSphere`, `ParaDrop`,
    /// `AmerParaDrop` and `SpyPlane`; an EVA line only for `IronCurtain`
    /// (`0x006CCF21`), `LightningStorm` (`0x006CCD81`) and `ChronoWarp`
    /// (`0x006CCD03`); a positional `[AudioVisual]` cue only for `ForceShield`
    /// (`0x006CD176`, from the *type's* own `StartSound=`) and `PsychicReveal`
    /// (`0x006CD7BF`); and both for `MultiMissile`, `PsychicDominator` and
    /// `GeneticConverter`. Carrying the type rather than a per-case flag is
    /// what lets the app read both the case index and `StartSound=` off one
    /// object, as `param_1[10]` does. ForceShield carries its activation
    /// through SuperWeaponActivation at the original earlier call site.
    SuperWeaponLaunched {
        owner: InternedId,
        sw_type: InternedId,
        /// Where the positional cue plays, always over the target cell; each
        /// launch case names its own coordinate. Cases with no positional
        /// cue carry the target cell's centre at Z 0, which nothing reads.
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// A `CreateRadarEvent` with no local-player test and no sound, on every
    /// client: each admits it on its own radar. Type 13 from
    /// `SuperClass::Launch` case 1 (`0x006CCF2F`), case 4 (`0x006CC4BE` at
    /// the Chronosphere's source, `0x006CC4D2` at its target), case 7
    /// (`0x006CCDD7`), a storm's start (`0x00539F89`) or a `NUKE` warhead's
    /// impact (`BulletClass::AI`, `0x00467EA7`); type 12 from a computer
    /// house's `RevealToAll=` building opening (`0x004468A8`).
    RadarEvent { radar: RadarEventRequest },
    /// A Lightning Storm started (`LightningStorm::Start @ 0x00539EB0`'s
    /// undeferred half) under `[General] LightningPrintText=` (`0x0053A014`;
    /// without it nothing is pushed): every client plays `StormSound` centred
    /// (`0x0053A044`) and posts `TXT_LIGHTNING_STORM` in the player's colours
    /// (`0x0053A076`). On retail data (`LightningDeferment=250`) this trails
    /// the launch's EVA line by the whole countdown.
    LightningStormBegan,
    /// A deferred Lightning Storm's countdown reached a multiple of 225
    /// (`LightningStorm::Process @ 0x0053AAD7`) under `LightningPrintText=`
    /// (`0x0053AAE9`; without it nothing is pushed): every client speaks
    /// `EVA_LightningStormCreated` (`0x0053AB11`) and posts
    /// `TXT_LIGHTNING_STORM_APPROACHING` (`0x0053AB40`).
    LightningStormApproaching,
    /// `owner`'s Lightning Storm was refused because a storm rages or counts
    /// down (`SuperClass::ClickFire @ 0x006CBAA7..0x006CBAC7`). The client
    /// whose player owns it (Fire_SW passes `this == PlayerPtr`,
    /// `0x004FAE8E`) posts `Msg:LightningStormActive`
    /// (`LightningStorm::PrintMessage @ 0x0053AE00`).
    LightningStormRefused { owner: InternedId },
    /// `owner`'s Psychic Dominator was refused because one is active
    /// (`0x006CBAD6..0x006CBAF6`). The client whose player owns it posts
    /// `Msg:DominatorActive` (`PsyDom::PrintMessage @ 0x0053B410`).
    PsychicDominatorRefused { owner: InternedId },
    /// A paid superweapon was ready but its House wallet cannot cover the
    /// configured cost. Only the owning client presents the configured EVA.
    SuperWeaponInsufficientFunds {
        owner: InternedId,
        sw_type: InternedId,
    },
    /// First occupant entered a CanBeOccupied building (cargo 0→1).
    /// Owner is the building owner at AddGarrisonOccupant time; civilian
    /// ownership transfer is reported separately from building reconciliation.
    /// App layer plays EVA_StructureGarrisoned if owner is local human.
    /// `type_id` is the occupied building, already resolved at the emit site.
    StructureGarrisoned {
        owner: InternedId,
        type_id: InternedId,
    },
    /// CheckAutoSellOrCivilian458200 abandoned an empty map garrison.
    /// House50B6F0 already admitted the old owner. The sound precedes radar
    /// admission; EVA plays only when CreateRadarEvent accepts the request.
    StructureAbandoned {
        owner: InternedId,
        radar: crate::sim::radar::RadarEventRequest,
        /// The abandoned building, already resolved at the emit site.
        type_id: InternedId,
    },
    /// First-occupant SFX from rulesmd [AudioVisual] BuildingGarrisonedSound.
    /// Positional cue gated on owner == local human, at the occupant's
    /// GetCoords (vt+0x48 on the infantryman, `0x005229C6..0x005229E0`).
    BuildingGarrisonedSfx {
        owner: InternedId,
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// Tank-bunker walls-up cue — emitted on install. App resolves to
    /// [AudioVisual] BunkerWallsUpSound (retail "TankBunkerUp"), at the
    /// bunker's Location (`BuildingClass::UpdateBunker @ 0x00458E50` state 5,
    /// `0x0045934A..0x0045936F`).
    BunkerWallsUp {
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// Tank-bunker walls-down cue — emitted on normal exit / clear teardown.
    /// App resolves to [AudioVisual] BunkerWallsDownSound (retail "TankBunkerDown"),
    /// at the bunker's Location (`ClearBunker @ 0x00459470`,
    /// `0x004594A2..0x004594C8`; `EmptyBunker @ 0x004595C0`,
    /// `0x004595E7..0x0045960D`).
    BunkerWallsDown {
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// A paratrooper left the paradrop plane (`AircraftClass::Drop_Payload`
    /// plays `[AudioVisual] ChuteSound=` at the plane's Location,
    /// `0x00415E21`).
    ChuteSound {
        world: crate::sim::anim_class::AnimWorldCoord,
    },
    /// An engineer entered a `BridgeRepairHut` and triggered bridge repair.
    /// Played at the hut BUILDING's Location, NOT the engineer's
    /// (`InfantryClass::PerCellProcess`, `0x00519BCE..0x00519C02`, on the
    /// building `Look_up_building_in_cell @ 0x0047C520` found at
    /// `0x00519B20`). App layer plays the
    /// spatial `[BridgeRepaired]` sound for everyone in range, gated on
    /// `rules.bridge_rules.repair_sound.is_some()`. `radar` is present when
    /// native House50B6F0 passed; the EVA line then waits on the client's
    /// non-drawing type-14 radar event creation/dedup gate.
    BridgeRepaired {
        world: crate::sim::anim_class::AnimWorldCoord,
        radar: Option<RadarEventRequest>,
    },
    /// Ares SPECIAL_PLACE10038ECA..10038EE0: automatic selection for a
    /// manual event failed. Only the owning client prints Message.CannotFire;
    /// its existing message-list transport owns the insert sound.
    AresSuperWeaponRefused {
        owner: InternedId,
        sw_type: InternedId,
    },
    /// Common Ares Launch10033201..10033242: Ready queue cleanup after FX.
    /// With `message`, every client also prints the type's Message.Launch
    /// (10033153), which a handler's NoMessage Flag skips. `sw_type` is the
    /// type whose SWTypeExt the Launch read (a PostClick Super's source); a
    /// handler whose Flags hold both NoMessage and NoCleanup sends none.
    AresSuperWeaponFinished {
        owner: InternedId,
        sw_type: InternedId,
        message: bool,
    },
    /// Common Ares Launch10032FC3: every client plays `sw_type`'s
    /// `EVA_Activated` (the PostClick source's for a PostClick Super),
    /// unless the handler's Flags hold NoEVA.
    AresSuperWeaponActivatedEva { sw_type: InternedId },
    /// A new type's AbortFire refused a click (SW_PsychicDominator
    /// 10070780): the owner's client prints the type's `Message.Abort`.
    AresSuperWeaponAborted {
        owner: InternedId,
        sw_type: InternedId,
    },
    /// A type-13 radar event only the clients whose player `owner` counts as
    /// an ally see (SW_NuclearMissile::Activate 10072FF0: "allies can see
    /// the target location before the enemy does").
    AresAlliedRadarEvent {
        owner: InternedId,
        radar: RadarEventRequest,
    },
    /// Ares SuperClass_Update_DrainMoney100358A0 refused a draining Super's
    /// money for a ControlledByHuman House: every client plays
    /// EVA.InsufficientFunds and prints the label in the player's colour.
    AresSuperWeaponDrainRefused {
        owner: InternedId,
        sw_type: InternedId,
    },
    /// A new type's deferred activation (UnitDelivery's state machine
    /// Update10075DE0, the Ares Lightning Storm's start 10036B00): every
    /// client prints Message.Activate.
    AresSuperWeaponActivated {
        owner: InternedId,
        sw_type: InternedId,
    },
    /// An Ares Lightning Storm's countdown reached a multiple of 225 under
    /// `Lightning.PrintText` (hook 53A6CF, 10037330): every client prints
    /// the type's Message.Launch, with no EVA line.
    AresLightningStormApproaching {
        owner: InternedId,
        sw_type: InternedId,
    },
}

impl SimSoundEvent {
    pub(crate) fn cloak_sound(sound_id: SoundName, position: &Position) -> Self {
        Self::CloakSound {
            sound_id,
            rx: position.rx(),
            ry: position.ry(),
            sub_x: position.sub_x(),
            sub_y: position.sub_y(),
            world_z_leptons: Self::world_z_leptons(position),
        }
    }

    pub(crate) fn wall_crushed(sound_id: SoundName, position: &Position) -> Self {
        Self::WallCrushed {
            sound_id,
            rx: position.rx(),
            ry: position.ry(),
            sub_x: position.sub_x(),
            sub_y: position.sub_y(),
            world_z_leptons: Self::world_z_leptons(position),
        }
    }

    pub(crate) fn voc_at(sound_id: SoundName, position: &Position) -> Self {
        Self::voc_at_for(sound_id, None, position)
    }

    /// `VocClass::PlayAt @ 0x007509E0` at an object's Location (`+0x9C`),
    /// context 0.
    pub(crate) fn voc_at_location(
        sound_id: SoundName,
        entity: &crate::sim::game_entity::GameEntity,
        terrain: Option<&ResolvedTerrainGrid>,
    ) -> Self {
        Self::VocAt {
            sound_id,
            audible_to: None,
            rx: entity.position.rx(),
            ry: entity.position.ry(),
            sub_x: entity.position.sub_x(),
            sub_y: entity.position.sub_y(),
            world_z_leptons: crate::sim::movement::ground_pose::object_location(entity, terrain).z,
        }
    }

    /// `VocClass::PlayAt @ 0x007509E0` for a source whose authoritative
    /// location is a retained world coordinate rather than raw Position.
    pub(crate) fn voc_at_world_coord(
        sound_id: SoundName,
        coordinate: crate::sim::anim_class::AnimWorldCoord,
    ) -> Self {
        let (rx, ry, sub_x, sub_y, _) = coordinate.to_cell_sub_z();
        Self::VocAt {
            sound_id,
            audible_to: None,
            rx,
            ry,
            sub_x,
            sub_y,
            world_z_leptons: coordinate.z,
        }
    }

    pub(crate) fn voc_at_for(
        sound_id: SoundName,
        audible_to: Option<[InternedId; 2]>,
        position: &Position,
    ) -> Self {
        Self::VocAt {
            sound_id,
            audible_to,
            rx: position.rx(),
            ry: position.ry(),
            sub_x: position.sub_x(),
            sub_y: position.sub_y(),
            world_z_leptons: Self::world_z_leptons(position),
        }
    }

    fn world_z_leptons(position: &Position) -> i32 {
        crate::sim::movement::ground_pose::position_world_coord(position).z
    }
}

/// The firer's own facing at the authoritative fire tick (the muzzle is
/// `SimFireEvent::fire_coord`).
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone)]
pub struct FireOriginSnapshot {
    pub facing: u8,
}

/// One shot, as combat resolved it at the authoritative fire tick. The world
/// constructs the shot's muzzle animation from it. Report playback uses the
/// separate, ordered `SimSoundEvent` stream at the retained `fire_coord`.
#[derive(Debug, Clone)]
pub struct SimFireEvent {
    /// Stable ID of the entity that fired.
    pub attacker_id: u64,
    /// Which weapon slot was used (Primary or Secondary).
    #[cfg(any(test, feature = "test-support"))]
    pub weapon_slot: WeaponSlot,
    /// Selected weapon section id.
    pub weapon_id: InternedId,
    /// Firing object's facing at the fire tick.
    #[cfg(any(test, feature = "test-support"))]
    pub facing: u8,
    /// Source facts from the fire tick, before burst/cooldown updates.
    #[cfg(any(test, feature = "test-support"))]
    pub origin_snapshot: FireOriginSnapshot,
    /// What was fired at — entity stable ID or ground cell coord.
    /// For projectile trajectory: Entity → look up entity position; Cell →
    /// use cell center as the destination.
    pub target: crate::sim::combat::TargetKind,
    /// Selected weapon report sound id, retained as shot metadata. Playback
    /// is submitted inline through `SimSoundEvent`, never replayed from here.
    pub report_sound_id: Option<InternedId>,
    /// The shot's fire coordinate in world leptons (`combat::fire_coord`): the
    /// bullet origin, the muzzle animation and the report sound share it.
    pub fire_coord: crate::sim::projectile::ProjectileCoord,
    /// `fire_coord.y` minus the firer coordinate's Y; a building's muzzle
    /// animation derives its `ZAdjust` from it.
    pub fire_offset_y: i32,
    /// The muzzle `AnimClass` type this shot constructs, if any: the weapon's
    /// `Anim=` by aim facing, or `OccupantAnim=` for an occupied building.
    pub muzzle_anim: Option<InternedId>,
    /// The firer is a building with occupants (native's `+0x408` count above
    /// zero), whichever weapon fired.
    pub occupied_building: bool,
    /// The firer's class: a building's flash is not attached to it.
    pub firer_category: EntityCategory,
}

#[cfg(any(test, feature = "test-support"))]
impl SimFireEvent {
    /// A primary-weapon shot by unit `attacker_id` from cell (0, 0), with no
    /// report, no muzzle animation and a zero fire coordinate.
    pub fn for_test(attacker_id: u64) -> Self {
        Self {
            attacker_id,
            weapon_slot: crate::sim::combat::combat_weapon::WeaponSlot::Primary,
            weapon_id: crate::sim::intern::test_intern("TESTWEAPON"),
            facing: 0,
            origin_snapshot: FireOriginSnapshot { facing: 0 },
            target: crate::sim::combat::TargetKind::Cell(0, 0),
            report_sound_id: None,
            fire_coord: crate::sim::projectile::ProjectileCoord::new(0, 0, 0),
            fire_offset_y: 0,
            muzzle_anim: None,
            occupied_building: false,
            firer_category: EntityCategory::Unit,
        }
    }
}
