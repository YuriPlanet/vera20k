//! The global `GeneralRules` block: its fields, the constructor defaults
//! they start from and the INI readers that fill them.

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::phobos_switch::phobos_read;
use crate::util::fixed_math::SimFixed;
use crate::util::native_random::unit_draw_threshold;
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};

use super::{
    AiIonCannonValues, AnimRef, DetailRules, DifficultyRules, GeneralBuildingTypes, ParadropList,
    PrismSupportRules, SpotlightRules, native_minutes_to_frames,
};

/// Global gameplay constants from `[General]` that affect vision, gap generators, etc.
#[derive(Debug, Clone)]
pub struct GeneralRules {
    /// `[General] MaxWaypointPathLength`, Rules+90. Constructor665725
    /// writes15; ReadGeneral671DB2..671DCC passes the prior signed value to
    /// ReadInteger5276D0 and stores it unchanged, without a clamp.
    pub max_waypoint_path_length: i32,
    /// General.TalkBubbleTime, Rules+1800; ctor6677A3=300 wall-clock buckets.
    pub talk_bubble_time: i32,
    /// `[General] FlashFrameTime=`, Rules `+0x88`: the constructor's 7
    /// (`0x00665711`), read by ReadInteger at `0x00671981` without a clamp.
    /// The radar combat flash's toggle period.
    pub flash_frame_time: i32,
    /// `[General] WeedCapacity=`, Rules `+0x17D0`: the constructor's 0
    /// (`0x006676FD`), read by ReadInteger at `0x00671962` without a clamp.
    /// A Weeder building's Tiberium pip maximum
    /// (`BuildingTypeClass::GetPipMax @ 0x0045ECE0`).
    pub weed_capacity: i32,
    /// `[General] RadarCombatFlashTime=`, Rules `+0x8C`: the constructor's 21
    /// (`0x0066571B`), read by ReadInteger at `0x006719A1` without a clamp.
    /// ReceiveDamage starts each Techno's radar combat flash with it.
    pub radar_combat_flash_time: i32,
    /// `[AudioVisual]` thresholds copied to the presentation frame-rate
    /// controller by Fill_In_Data6850F5 and the radar movie transitions.
    pub detail: DetailRules,
    /// `[AudioVisual] PoseDir=`, Rules `+0x44`: the constructor's 0
    /// (`0x006656B9`), then ReadInteger (`0x00669268`) stored raw, without
    /// DeployDir's shift. An aircraft with no radio contact and no passengers
    /// lands facing it (`AircraftClass::Landing_Direction @ 0x0041B760`,
    /// which Fly shifts left by 13).
    pub pose_dir: i32,
    /// Rules+48, constructor6656BD zero; AudioVisual669272..66929B reads
    /// DeployDir with ReadInteger5276D0 then wrapping SHL5, after PoseDir.
    /// Jumpjet54C765 takes the low byte; retain the raw signed stored dword.
    pub deploy_dir: i32,
    /// Edge-scroll speed scale from `[AudioVisual] ScrollMultiplier=`.
    /// Stock YR uses `.07`; this is app-facing presentation state.
    pub scroll_multiplier: f64,
    /// Outcome-announcement grace period in minutes from `[AudioVisual]
    /// SavourDelay=`. HouseClass converts this to frames with `ftol(value*900)`
    /// before routing victory/defeat into scenario teardown.
    pub savour_delay_minutes: f64,
    /// Per-tick Spark gravity AND hover-bob amplitude, from `[AudioVisual]
    /// Gravity=` (NOT `[General]` — stock rulesmd.ini defines it under
    /// [AudioVisual], value 6; the engine's code default is 3). Native stores a
    /// signed integer and converts to f32 at the behavior-3 tick boundary.
    /// Also the ballistic gravity: `WeaponTypeClass::GetSpeed @ 0x00773070`
    /// derives every `ROT=0` launch speed from it.
    pub gravity: i32,
    /// Rules+7AC, ReadGeneral66F2AF: full signed wind-table index.
    pub(super) wind_direction: i32,
    /// `[General] VeteranSight=` — `RulesClass+0x680`, a double.
    ///
    /// gamemd-derived: `TechnoClass::UpdateReveal @ 0x0070AF50` MULTIPLIES the
    /// elevation-scaled sight by it (`0x0070B095..0x0070B09F`, `FILD; FMUL;
    /// ftol`), only for a `SIGHT`-ability holder, and only when the value is
    /// not exactly `0.0` (`FCOMP` gate at `0x0070B088`). Stock `0.0` therefore
    /// disables the bonus. The constructor default is UNCHECKED.
    pub veteran_sight: f64,
    /// `[General] VeteranCombat=` — `RulesClass+0x670`. `TechnoClass::Fire_At`
    /// multiplies the folded firepower damage by it at `0x006FE3C8..0x006FE3D8`
    /// for a `FIREPOWER`-ability holder. Constructor default UNCHECKED.
    pub veteran_combat: f64,
    /// `[General] VeteranSpeed=` — `RulesClass+0x678`.
    /// `FootClass::GetCurrentSpeed @ 0x004DB1A0` multiplies the truncated type
    /// speed by it at `0x004DB1F1..0x004DB200` for a `FASTER`-ability holder.
    /// Constructor default UNCHECKED.
    pub veteran_speed: f64,
    /// Phobos `[General] ProneSpeed.Crawls=` (default 0.67): the multiplier
    /// `InfantryClass::GetMovementSpeed @ 0x00521D80` applies to a prone
    /// infantryman whose type has `Crawls=`.
    #[cfg(feature = "phobos")]
    pub prone_speed_crawls: f64,
    /// Phobos `[General] ProneSpeed.NoCrawls=` (default 1.5): the same
    /// multiplier for a prone type with `Crawls=no`.
    #[cfg(feature = "phobos")]
    pub prone_speed_no_crawls: f64,
    /// `[General] VeteranROF=` — `RulesClass+0x690`, read at `0x0066EF61`.
    /// `TechnoClass::GetROF @ 0x006FCFA0` multiplies the jittered reload by it
    /// at `0x006FD136..0x006FD145` for a `ROF`-ability holder. Constructor
    /// default 1.0 (`0x00665F86`).
    pub veteran_rof: f64,
    /// Rules+1538, stride50: Easy, Normal, Difficult in native index order.
    /// ReadDifficulty66D270 skips an absent whole section and resets every
    /// missing key of a present section to a literal default. The native
    /// constructor leaves absent whole rows undefined; VERA's cold fallback
    /// uses the reader defaults. Retail supplies all three rows.
    pub difficulty_rows: [DifficultyRules; 3],
    /// Rules+1418: [General] GameSpeedBias, read at670B18/670B33. This scales
    /// the stored Groundspeed/Airspeed/BuildTime scalar biases; their native
    /// active gameplay getters use separate per-category country factors.
    pub game_speed_bias: f64,
    /// Receiver-side divisor selected by the rank-specific `STRONGER`
    /// ability (`VeteranArmor=` in `[General]`).
    pub veteran_armor: f64,
    /// `[General] CurleyShuffle=` — `RulesClass+0x17E1`, read at `0x0066FD4A`
    /// (ReadBool with the field as its default; the constructor writes 0).
    /// `AircraftClass::Mission_Attack` reads it in states 4 and 5
    /// (`0x004183C3`, `0x00418671`, `0x00418733`, `0x00418782`): a helicopter
    /// that fired or missed goes back to state 1 and picks a new firing spot.
    /// Retail: yes.
    pub curley_shuffle: bool,
    /// `[General] EnableAIBuildLimitation=` — the older extension build's
    /// switch (NPExt ext173, manual `AI克隆生产`'s predecessor): with yes, the
    /// computer's production attempt checks the type's own `BuildLimit=`
    /// (`Type+0x3B8`) like a production event does, and a plan at its limit
    /// clears its choice and skips. Read by that build's `ReadGeneral` hook
    /// `0xC2D132..0xC2D151` into `Rules+0x641`, default the current field, so
    /// the constructor clears that byte through the +640 DWORD store
    /// (`0x665F09`), so absent means no. NPSE #359 dropped this key and
    /// reads per-type AIBuildLimit (`0xC27249..0xC2725A`) instead. Both
    /// generations use factory_ai's shared tracked/produced comparison.
    pub enable_ai_build_limitation: bool,
    /// `[General] DisableParallelAIQueues=` — an NPSE 2.0 extension switch
    /// read into `Rules+0x2D` by `ReadGeneral` C30303..C3031A through
    /// ReadBooleanByte5295F0 with the current byte as default. The Rules
    /// constructor's DWORD store at 66569B clears +2D, so absent means no.
    /// With yes, Factory_AI C2F07C reserves by produced class, splitting
    /// vehicles alone on Naval. HouseAiProduction owns the reservation and
    /// its class-specific exit/abandon release phases. Default no keeps cloning.
    /// NPatch Extended (ext173) names the same global switch
    /// `[General] DisableAIParallelProduction=`: its `ReadGeneral` hook reaches
    /// the reader at `0xC2BDDA..0xC2BDE7`, which writes `Rules+0x640`,
    /// immediately before `EnableAIBuildLimitation` at `Rules+0x641`, and
    /// `0xC2D482` gates the same five counters. The 2022-era revision that
    /// PR #2877 discusses sets the name per type; only `[General]` is consulted
    /// here, so a type-section key stays unread instead of driving this field.
    pub disable_parallel_ai_queues: bool,
    /// `[General] RepairRate=` in minutes — `RulesClass+0x16E0`.
    ///
    /// The self-heal pulse (`FUN_0070BE80`) divides the frame counter by
    /// `ftol(RepairRate * 900)` (`0x0070BEFE..0x0070BF0A`, constant 900.0 at
    /// `0x007E27F8`); stock `.016` gives a 14-frame pulse.
    pub repair_rate_minutes: f64,
    /// `[General] SelfHealInfantryFrames=` — `RulesClass+0x30`, read by the
    /// General reader at `0x0066D530`. The house pulse of
    /// `TechnoClass::AI_Update @ 0x006FA8E2` divides the global frame counter
    /// by it (`idiv`) and runs only on an exact remainder of zero.
    /// Ctor6656A7 defaults1000; ReadInt66E6F3 retains it. Stock50.
    pub self_heal_infantry_frames: i32,
    /// `[General] SelfHealInfantryAmount=` — `RulesClass+0x34`, read by the
    /// same reader. `HouseClass::GetInfSelfHealStep @ 0x0050D9E0` multiplies it
    /// by the house's infantry count (`+0x164`). Ctor6656AA defaults1;
    /// ReadInt66E70D retains it. Stock20.
    pub self_heal_infantry_amount: i32,
    /// `[General] SelfHealUnitFrames=` — `RulesClass+0x38`, same reader. The
    /// unit pulse of `TechnoClass::AI_Update @ 0x006FA7EE` divides the frame
    /// counter by it. Ctor6656AD defaults1000; ReadInt66E727 retains it. Stock75.
    pub self_heal_unit_frames: i32,
    /// `[General] SelfHealUnitAmount=` — `RulesClass+0x3C`, same reader.
    /// `HouseClass::GetUnitSelfHealStep @ 0x0050D9F0` multiplies it by the
    /// house's unit count (`+0x168`). Ctor6656B0 defaults1;
    /// ReadInt66E740 retains it. Stock5.
    pub self_heal_unit_amount: i32,
    /// `[General] VeteranRatio=` — how many times its own cost an object must
    /// destroy to gain one rank. `RulesClass+0x668`, read at `0x0066EEB0`.
    pub veteran_ratio: f64,
    /// `[General] VeteranCap=` — the accumulator clamp. `RulesClass+0x698`,
    /// read at `0x0066EF94`. Stock `2`, which is exactly the elite threshold,
    /// so elite is terminal.
    pub veteran_cap: f64,
    /// `[AI] ComputerBaseDefenseResponse=` (ReadAI673E41..673E61).
    /// The active House responder forms its signed/wrapping budget as
    /// `attacker ThreatPosed * this value` (7080BA..7080C4).
    pub computer_base_defense_response: i32,
    /// Signed `[General] MaximumBuildingPlacementFailures=` at `Rules+0xE48`.
    /// Native constructor default is `5`; both active retail rules files
    /// override it with `3`. Building exit compares strictly after incrementing;
    /// negative mod values remain literal.
    pub maximum_building_placement_failures: i32,
    /// `[General] PlacementDelay=` in minutes (`Rules+0x5F8`, ReadDouble at
    /// `0x0066F208`, constructor .05 at `0x00665E8C`): how long a computer
    /// Construction Yard waits after a blocked placement before it tries its
    /// finished building again ([`Self::placement_delay_frames`]).
    pub placement_delay: f64,
    /// `[AI] AttackDelay=`, Rules+0x10A8. The constructor supplies 5.0;
    /// ReadAI673977..67399E reads a double with that current-field default.
    /// House construction scales its one Scenario ranged draw by this value.
    pub attack_delay: f64,
    /// `[General] CampaignMoneyDeltaEasy/Hard`, ReadInteger at
    /// 0x0067015B/0x0067017A with current defaults 5000/-5000. Campaign
    /// House credits apply the selected signed delta after Credits*100.
    pub campaign_money_delta_easy: i32,
    pub campaign_money_delta_hard: i32,
    /// `[General] AIAlternateProductionCreditCutoff=` (`Rules+0x1300`,
    /// ReadInt at `0x0066FDFB`, constructor 1000 at `0x0066703B`): the credits
    /// below which the computer's production mode leaves its normal state.
    pub ai_alternate_production_credit_cutoff: i32,
    /// `[General] AIRestrictReplaceTime=` (`Rules+0xDF0`, ReadInt at
    /// `0x00670110`, constructor 500 at `0x006668D4`; retail 400): the frames
    /// after a building of the house is attacked during which the computer
    /// replans a lost building only if it is armed, a wall or a power plant.
    pub ai_restrict_replace_time: i32,
    /// `[General] TeamDelays=` (`Rules+0x1158`, items `+0x115C`; the
    /// `0x00475D70` vector read at `0x0066FD8B`, constructor empty): the frames
    /// between a computer house's team creation passes, by House difficulty
    /// (`sim::ai_team_creation`). Retail: 2000,2500,3500.
    pub team_delays: Vec<i32>,
    /// `[General] TotalAITeamCap=` (`Rules+0x13C8`, items `+0x13CC`, read at
    /// `0x0066FF30`): the teams a computer house may own before it stops
    /// creating more. Retail: 30,30,30.
    pub total_ai_team_cap: Vec<i32>,
    /// `[General] MinimumAIDefensiveTeams=` (`Rules+0x1390`, items `+0x1394`,
    /// read at `0x0066FEBD`). Retail: 1,1,1.
    pub minimum_ai_defensive_teams: Vec<i32>,
    /// `[General] MaximumAIDefensiveTeams=` (`Rules+0x13AC`, items `+0x13B0`,
    /// read at `0x0066FEF6`). Retail: 2,2,2.
    pub maximum_ai_defensive_teams: Vec<i32>,
    /// `[General] UseMinDefenseRule=` (`Rules+0x17F3`, GetBool at
    /// `0x0066FF60`, constructor 1): while a house owns fewer base-defense
    /// teams than the minimum, only base-defense AI triggers qualify.
    pub use_min_defense_rule: bool,
    /// `[General] FillEarliestTeamProbability=` (`Rules+0x13F0`, items
    /// `+0x13F4`, read at `0x0066FE83`): the percent chance a unit chooser
    /// fills the earliest forming team's need. Retail: 100,100,100.
    pub fill_earliest_team_probability: Vec<i32>,
    /// `[General] DissolveUnfilledTeamDelay=` (`Rules+0x1190`, ReadInt at
    /// `0x0066FF80`, constructor 5000): the frames an empty team lasts in a
    /// multiplayer game (`TeamClass::AI @ 0x006E9140`).
    pub dissolve_unfilled_team_delay: i32,
    /// `[General] AISafeDistance=` (`Rules+0xD74`, ReadInteger at
    /// `0x0066FFA7`, constructor 8 at `0x0066682A`): the cells from its base
    /// centre a team gathers at on script action 54 (`0x006EFB69`).
    /// Retail: 20.
    pub ai_safe_distance: i32,
    /// `[AI] PatrolScan=` (`Rules+0x1150`, ReadDouble6739BA inside
    /// ReadAI672AE0). Constructor666E09/666E1A stores the literal double
    /// 0.016; each reached AI pass retains its prior value as the default.
    /// Team Script16 uses the signed global frame modulo ftol(value*900),
    /// without a clamp or a per-team timer. Source: Steam gamemd,
    /// tools/team_recruit_oracle.py --patrol.
    pub patrol_scan_minutes: NativeF64Bits,
    /// `[General] AITriggerSuccessWeightDelta=` (`Rules+0xC0`, ReadDouble at
    /// `0x006718EC`, constructor 1.0; retail 20).
    pub ai_trigger_success_weight_delta: NativeF64Bits,
    /// `[General] AITriggerFailureWeightDelta=` (`Rules+0xC8`, ReadDouble at
    /// `0x00671913`, constructor -1.0; retail -50).
    pub ai_trigger_failure_weight_delta: NativeF64Bits,
    /// `[General] AITriggerTrackRecordCoefficient=` (`Rules+0xD0`, ReadDouble
    /// at `0x0067193A`, constructor 1.0; retail 1).
    pub ai_trigger_track_record_coefficient: NativeF64Bits,
    /// `[General] HarvestersPerRefinery=` (`Rules+0x1358`, items `+0x135C`,
    /// read at `0x006705D3`): the harvesters a computer house wants for each
    /// refinery. Retail: 2,2,1.
    pub harvesters_per_refinery: Vec<i32>,
    /// `[General] AIMinorSuperReadyPercent=` (`Rules+0xD70`, a float read
    /// through ReadDouble at `0x0066FFC4`, constructor 0.8f): how charged an
    /// Iron Curtain or Chronosphere must be for an AI trigger to count it.
    pub ai_minor_super_ready_percent: NativeF32Bits,
    /// `[General] BaseDefenseDelay=` in minutes. A strict responder-budget
    /// overshoot arms the attacker cooldown for `ftol(value * 900)` frames.
    pub base_defense_delay_minutes: f64,
    /// `[General] SuspendPriority=`. Teams owned by the attacked House whose
    /// signed priority is lower than this value are suspended before scanning.
    pub suspend_priority: i32,
    /// `[General] SuspendDelay=` in minutes. Suspended TeamClass instances arm
    /// their native timer for `ftol(value * 900)` frames.
    pub suspend_delay_minutes: f64,
    /// Leptons of elevation per +1 sight cell (LeptonsPerSightIncrease=).
    /// 256 leptons = 1 z-level in RA2. 0 disables the elevation bonus.
    pub leptons_per_sight_increase: i32,
    /// Gap Generator effect radius in cells (GapRadius=). Default 10.
    pub gap_radius: i32,
    /// Height-based LOS obstruction (RevealByHeight= in [General]).
    /// When true, terrain 4+ levels above the viewer at the midpoint blocks sight.
    /// Default true (the standard RA2/YR setting).
    pub reveal_by_height: bool,
    /// `[AudioVisual] AllyReveal=` (`Rules+0x17E7`, ReadBool at `0x0066B318`
    /// over the constructor's 1, `0x0066773F`): an allied house's reveals map
    /// the player's cells too (the house gate of `MapClass::RevealArea @
    /// 0x005678E0`, `0x00567AEF`). Retail yes.
    pub ally_reveal: bool,
    /// `[MultiplayerDialogSettings] AlliesAllowed=` (`Rules+0x14B5`, ReadBool
    /// at `0x0067215B` in `0x00671EA0` over the constructor's 0,
    /// `0x006672D5`): Computer_Paranoid's gate (`0x0050164D`,
    /// `sim::house_diplomacy`). Retail RULESMD says no; the Battle mode's
    /// MPBATTLEMD layers yes over it.
    pub allies_allowed: bool,
    /// `[MultiplayerDialogSettings] AllyChangeAllowed=` (`Rules+0x14BB`,
    /// ReadBool at `0x00672175` over the constructor's 1, `0x00667202` /
    /// `0x006672F9`). Only the two ALLY producers read it (`0x00731840`,
    /// `0x007318F0`). Retail RULESMD says yes; Team Game's MPTEAMMD says no.
    pub ally_change_allowed: bool,
    /// `[AI] CompEasyBonus=`: Rules+17E3 starts true in constructor667727.
    /// ReadAI673CFD..673D1D uses ReadBool5295F0 with the retained byte,
    /// after AirstripLimit and before Paranoid, independently of General.
    /// CreateHouses688293..6882B9 decrements positive AI difficulty when
    /// the admitted human vector contains more than one node, observers included.
    pub comp_easy_bonus: bool,
    /// `[AI] Paranoid=` (`Rules+0x17E0`, ReadBool at `0x00673D23..0x00673D43`
    /// in ReadAI over the constructor's 1, `0x00667715`): a defeated computer
    /// house at `MaxIQLevels` (`0x004FC0FF`), and a human house's MakeAlly
    /// (`0x004F9C5F`), run Computer_Paranoid. Retail yes.
    pub paranoid: bool,
    /// Low byte of `CliffBackImpassability=` in `[General]`, as
    /// `read_cliff_back_impassability` reads it. Byte 0 skips the scan; only
    /// byte 2 can write Rock.
    pub cliff_back_impassability: u8,
    /// `[General] TunnelSpeed=` (`Rules+0x20`): ReadDouble at `0x0066EDA9`
    /// over the constructor's 1.0 (`0x00665685..0x00665688`); retail 1. The
    /// Tunnel locomotor scales its sink and rise speed by it and divides its
    /// dig-in time by it (`movement::tunnel_movement`). Kept as `SimFixed`,
    /// whose rounding of a non-dyadic value can move a truncated product by
    /// one.
    pub tunnel_speed: SimFixed,
    /// Rules+598: ReadDouble at 66EC37, constructor .25 at 665DBD.
    /// Bullet guidance uses sin((signed native ID + frame) % 15 * 2pi/15).
    pub missile_rot_var: f64,
    /// Rules+5A0: ReadInt at 66EC57, constructor 500 at 665DC9.
    /// Targetless guided flight detonates when old Object height reaches it.
    pub safety_altitude: i32,
    /// Rules+1863, AudioVisual66B77D ReadColorRGB. Any nonzero channel
    /// replaces ObjectType ART LineTrailColor on trail construction.
    pub line_trail_color_override: [u8; 3],
    /// Rules+1866, AudioVisual66B7A7 ReadColorRGB. SpawnRadBeam6FD752
    /// copies this RGB for an admitted Temporal weapon's mode1 beam.
    /// Constructor66785E/667864/66786A initializes all three bytes to zero.
    /// Original controls: tools/procedural_drawing_oracle/chrono_beam.md.
    pub chrono_beam_color: [u8; 3],
    /// Default cruise altitude for Fly-locomotor aircraft (FlightLevel= in [General]).
    /// Fallback 500 leptons matches the engine constructor default; retail
    /// rulesmd.ini always supplies its own (1500), so the fallback only fires
    /// for a non-retail INI missing the key. A type's own `FlightLevel=`
    /// overrides it through `ObjectType::flight_level`.
    pub flight_level: i32,
    /// `[General] AttackingAircraftSightRange=` (`Rules+0x18`, ReadInt at
    /// `0x00670F2E` over the constructor's 5, `0x00665682`): the radius
    /// `AircraftClass::Fire_At` reveals around the current player's shooting
    /// aircraft when its Location, a probe near it or its target is shrouded
    /// (`0x0041651D..0x00416595`). Retail 2.
    pub attacking_aircraft_sight_range: i32,
    /// Rules+420, [JumpjetControls] CruiseHeight. Object5F4260 uses this
    /// global threshold; linked Jumpjets instead use their own +2C height.
    pub display_cruise_height: i32,
    /// VERA: `[JumpjetControls] SmoothFacing=` (default no). When set, a
    /// Jumpjet's body no longer snaps to its locomotor's stale heading on
    /// take-off and turns at its ROT for an Unload
    /// ([docs/jumpjet-smooth-facing.md]).
    pub jumpjet_smooth_facing: bool,
    /// VERA: `[JumpjetControls] LandNearby=` (default 0, at most 8). A
    /// Jumpjet refused its landing cell lands on the nearest clear cell
    /// within this many cells instead of hovering
    /// ([docs/jumpjet-land-nearby.md]).
    pub jumpjet_land_nearby: i32,
    /// VERA: `[General] AINearestEnemy=` (default no). When set, a computer
    /// house picks the nearest enemy base instead of the first house in the
    /// array, and equal anger goes to the nearer house
    /// ([docs/ai-nearest-enemy.md]).
    pub ai_nearest_enemy: bool,
    /// `[General] HoverHeight=`, Rules+0x5CC: the Hover altitude controller's
    /// cruise height in leptons (0x00513D20). Constructor 120 (0x00665E2B).
    pub hover_height: i32,
    /// `[General] HoverBob=`, Rules+0x5D0 double: the bob period in minutes
    /// (0x00513E1A). Constructor 30.0 (0x00665E35/3B).
    pub hover_bob: NativeF64Bits,
    /// `[General] HoverBoost=`, Rules+0x5D8 double: SpeedMult for two equal
    /// queued path words (0x00516141). Constructor 1.3 (0x00665E45/54).
    pub hover_boost: NativeF64Bits,
    /// `[General] HoverAcceleration=`, Rules+0x5E0 double, minutes: the ramp
    /// step is `1 / (value * 900)` (0x0051617F). Constructor 0.03 (0x00665E63).
    pub hover_acceleration: NativeF64Bits,
    /// `[General] HoverBrake=`, Rules+0x5E8 double, minutes (0x005161B7).
    /// Constructor 0.03 (0x00665E6F).
    pub hover_brake: NativeF64Bits,
    /// `[General] HoverDampen=`, Rules+0x5F0 double (0x00513F21). Constructor
    /// 0.8 (0x00665E7B/81).
    pub hover_dampen: NativeF64Bits,
    /// Descent rate cap for parachuted units, in leptons/tick (signed).
    /// Per gamemd, the rate field accumulates by `-1` per tick and clamps
    /// to this value. Default `-3` matches `[General] ParachuteMaxFallRate=-3`.
    /// Negative = falling.
    pub parachute_max_fall_rate: i32,
    /// ObjectAI5F3FBC's non-parachute cap, Rules+7BC. Constructor66616D
    /// initializes -100; ReadGeneral66F32E..350 reads the current value
    /// as the default for [General] NoParachuteMaxFallRate via ReadInt5276D0.
    pub no_parachute_max_fall_rate: i32,
    /// Paradrop trigger radius in leptons. From `[General] ParadropRadius=`.
    /// Default 1024 (~4 cells). Distance to target at which the carrier aircraft
    /// reveals fog + transitions to the overfly mission.
    pub paradrop_radius: i32,
    /// Parsed `[General] Parachute=` value (uppercased AnimType name, e.g.
    /// "PARACH"), `None` if unset or empty: the canopy the simulation attaches
    /// to a dropped object. Its timing is the AnimType's own art.
    pub parachute_shp: Option<String>,
    /// Parsed `[General] VehicleParachute=` value (uppercased AnimType name),
    /// read only with the explicit NPatch capability.
    /// `None` if unset or empty: the NPSE canopy an airdropped non-infantry
    /// object takes instead of `Parachute=` (`ObjectClass::Paradrop`'s
    /// `0x5F5ADD` hook, cave `0xC2B0E4`..`0xC2FA8A`: infantry keeps
    /// `Parachute=`, anything else takes this when the Rules+0x400 pointer
    /// is set — the NPSE reader stores it there, `0xC31005..0xC31017` — and
    /// `Parachute=` otherwise). Read in the General reader with the 0x80
    /// string capacity; an empty read retains the previous pointer, while
    /// `none` clears it through AnimTypeClass::FindOrAllocate @ 0x428B80.
    pub vehicle_parachute: Option<String>,
    /// `[General] AmerParaDropInf=`/`AmerParaDropNum=` (`+0xC04`/`+0xC20`):
    /// the American Paradrop's planes.
    pub amer_paradrop: ParadropList,
    /// `AllyParaDropInf=`/`AllyParaDropNum=` (`+0xC3C`/`+0xC58`): the Allied
    /// side's Paradrop, and the Spy Plane's plane count.
    pub ally_paradrop: ParadropList,
    /// `SovParaDropInf=`/`SovParaDropNum=` (`+0xC74`/`+0xC90`).
    pub sov_paradrop: ParadropList,
    /// `YuriParaDropInf=`/`YuriParaDropNum=` (`+0xCAC`/`+0xCC8`).
    pub yuri_paradrop: ParadropList,
    /// `AnimToInfantry=` (`+0xCE4`), the InfantryTypes a `MakeInfantry=`
    /// anim's end picks from, from the native processed rules (empty from
    /// the constructor; retail `BRUTE`).
    pub anim_to_infantry: Vec<String>,
    /// Unit types that count as a player's home when no buildings remain.
    /// Parsed from `[General] BaseUnit=`. Stock YR: AMCV, SMCV, PCV.
    pub base_unit_types: Vec<String>,
    /// `[General] MultiplayerAICM=` — `RulesClass+0x1304` int list read by
    /// `RulesClass__ReadGeneral` at `0x00670512..0x0067053B`. Indexed directly
    /// by `HouseClass+0x184` difficulty (0 = Hard, 1 = Normal, 2 = Easy) in
    /// `ScenarioClass__Post_Map_Init @ 0x00686A52..0x00686A5E`; each entry is a
    /// percentage of the AI house's opening balance granted once at match start.
    /// Stock `rulesmd.ini:88` is `400,0,0`. The constructor default is an empty
    /// vector (`0x00667034`), which native would index out of bounds; VERA treats
    /// a missing entry as 0 (no grant).
    pub multiplayer_ai_cm: Vec<i32>,
    /// `[General] AIHateDelays=`: raw frame counts in Hard/Normal/Easy order.
    /// RulesClass::ReadGeneral0066FDB1..0066FDD9 reads the retained vector
    /// through ReadINI_IntVector00475D70. Post_Map_Init00686A12..00686A2B
    /// starts each nonhuman, nonpassive House's enemy-search timer from it.
    /// The native constructor leaves the vector empty, not at retail30,50,70.
    pub(super) ai_hate_delays: Vec<i32>,
    /// Aircraft types in native Rules `PadAircraft` order. BuildingType's
    /// virtual value calculation reads the first two entries for the bundled
    /// helipad-cost branch.
    pub pad_aircraft_types: Vec<String>,
    /// `SeparateAircraft=`. False means the first pad building's value includes
    /// the average cost of the first two `PadAircraft` entries.
    pub separate_aircraft: bool,
    /// `[General] PrismType=` (Rules `+0x498`): the BuildingType whose
    /// Mission_Attack forwards its charge instead of firing
    /// (`sim::world::techno_ai::building_missions`).
    pub prism_type: Option<String>,
    /// Ordered [General] DropZoneAnim reference at Rules+147C. Constructor
    /// 66723F clears it;66E5D5 retains an empty read and resolves nonempty names.
    pub drop_zone_anim: Option<String>,
    /// [AudioVisual] DropZoneRadius, stored in leptons at+14BC. Constructor
    /// 6672FF:1024;66B385 uses ReadRange474620 with the retained value.
    pub drop_zone_radius: i32,
    /// `[General] PrerequisiteProcAlternate=` (Rules `+0x400`): the UnitType
    /// whose on-map units also meet a `PROC` prerequisite
    /// (`sim::production::can_build`).
    pub prerequisite_proc_alternate: Option<String>,
    /// `PrismSupportModifier/Max/Delay/Duration=`.
    pub prism_support: PrismSupportRules,
    /// `GDIGateOne=`, `GDIGateTwo=`, `NodGateOne=`, `NodGateTwo=`,
    /// `WallTower=` (Rules `+0x86C..+0x87C`) and the four power plants
    /// (`+0x89C..+0x8A8`).
    pub building_types: GeneralBuildingTypes,
    /// `GrowthRate=` in [General], in minutes. Fallback 2.0 matches the engine
    /// constructor default; retail rulesmd.ini always supplies its own (5), so
    /// the fallback only fires for a non-retail INI missing the key. Parsed for
    /// rules fidelity only: VERA's growth runs on the per-Tiberium `Growth=`
    /// timers and nothing reads this value.
    pub growth_rate_minutes: f32,
    /// Animation played when a unit warps in (WarpIn= in [General]).
    pub warp_in: AnimRef,
    /// Animation played when a unit warps out (WarpOut= in [General]).
    pub warp_out: AnimRef,
    /// Animation for chrono-erasing a unit (WarpAway= in [General]).
    pub warp_away: AnimRef,
    /// Sparkle particles during chrono teleport (ChronoSparkle1= in [General], YR feature).
    pub chrono_sparkle1: AnimRef,
    /// Wake animation spawned behind ships moving on water (Wake= in [General]).
    pub wake: AnimRef,
    /// Multiplayer move-command feedback animation (MoveFlash= in [General]).
    pub move_flash: AnimRef,
    /// `[General] Behind=` (`Rules+0xB8`): the indicator Anim a Techno
    /// behind a building takes (`TechnoClass::AI_Update @ 0x006FA2D3`).
    /// The ordered processor's General read (`0x0066E4F5..0x0066E513`,
    /// AnimType factory `0x00428B80`) publishes it; the constructor leaves it
    /// null (`0x00665765`).
    pub behind: Option<String>,
    /// Fatal infantry animation bindings indexed by Warhead `InfDeath`.
    /// Slots 3/4/6..10 come from `[General]`; slot 5 is the second declared
    /// `[Animations]` entry. Slots 0..2 intentionally have no external anim.
    pub infantry_death_anims: [Option<String>; 11],
    /// `[General] AttackCursorOnDisguise`, Rules+D6C: constructor66681A
    /// clears it; ReadGeneral66FC36..66FC50 retains the current byte.
    /// The shared manual-attack query owns the disguise and blink exceptions.
    pub attack_cursor_on_disguise: bool,
    /// `[General] AllowShroudedSubteranneanMoves`, Rules+17EF. Constructor
    /// 665650 clears it at66776F; ReadGeneral66EB03..66EB1C retains the
    /// current bool. Foot object-click4D785F consumes this shroud gate.
    pub(super) allow_shrouded_subterannean_moves: bool,
    /// `[General] DefaultMirageDisguises=` selection pool, in source order.
    pub default_mirage_disguises: Vec<String>,
    /// General AlliedDisguise/SovietDisguise/ThirdDisguise (Rules
    /// `+0xD58/+0xD5C/+0xD60`), retained by the native scalar reader
    /// 66FB94..66FBEB. Infantry GetDisguise522640 uses the current player's
    /// side, independently of the actor's owner; a `CanDisguise=`/
    /// `PermaDisguise=` object of each side starts wearing its side's entry
    /// (`TechnoClass::Init_Managers`, `0x006F423F..0x006F4271`). Stock E1,
    /// E2 and INIT.
    pub(crate) infantry_disguises: [Option<String>; 3],
    /// General SecretInfantry/SecretUnits/SecretBuildings, retained native identities.
    pub(super) secret_lab_choices: [Vec<String>; 3],
    /// `[General] InfantryBlinkDisguiseTime=` reveal duration in frames.
    pub infantry_blink_disguise_time: i32,
    /// Whether the attack cursor appears on trees/terrain
    /// (`TreeTargeting=` in `[CombatDamage]`).
    /// Default false in vanilla RA2.
    pub tree_targeting: bool,
    /// Signed Terrain Strength fallback. Rules ctor666DF8 stores25;
    /// General671DD2..671DF1 reads TreeStrength (retail200). Native reader
    /// evidence: tools/rules_oracle/bridge_landing_inputs.
    pub tree_strength: i32,
    /// Health ratio threshold below which the bar turns yellow (ConditionYellow= in [AudioVisual]).
    /// Default 0.5 (50%).
    pub condition_yellow: f64,
    /// Health ratio threshold below which the bar turns red (ConditionRed= in [AudioVisual]).
    /// Constructor default0.5 at667568..66756E; retail authors0.25.
    /// Native reader evidence: tools/rules_oracle/bridge_landing_inputs.
    pub condition_red: f64,
    /// `[General] CloakingStages=` — native signed progress divisor. The
    /// constructor and stock rules both use 9.
    pub cloaking_stages: i32,
    /// `[General] CloakDelay=` converted from minutes with native truncation
    /// toward zero (`ftol(minutes * 900)`). Stock `.02` becomes 18 frames.
    /// Rules66716C..667172 seeds0; ReadGeneral670AEB..670B0C retains it.
    pub cloak_delay_frames: i32,
    /// `[AudioVisual] CloakSound=` resolved by the app audio registry when a
    /// cloak transition requests positional playback. Stock YR binds
    /// `NavalUnitEmerge`; the native constructor's invalid Voc index is silence
    /// when the key is absent or cannot resolve.
    pub cloak_sound: Option<String>,
    /// Ares RulesExt+38/+3C nullable `[AudioVisual] DecloakSound=`
    /// (NullableIdx<Voc>1000E9D0 in LoadAfterTypeData1002A800), bound by the
    /// fixed-catalog sound binder. Unset falls back to `cloak_sound`.
    pub(crate) ares_decloak_sound: Option<Option<String>>,
    /// `[AudioVisual] LeaveBioReactorSound=` is RulesClass+0x274, read by
    /// `RulesClass::ReadAudioVisual @ 0x006691E0` and played by the successful
    /// InfantryAbsorb arm of `BuildingClass::Mission_Unload @ 0x0044D880`.
    /// `None` is the constructor's invalid Voc index; the fixed SOUNDMD binder
    /// resolves the name and retains the prior valid index across rules passes.
    pub leave_bio_reactor_sound: Option<String>,
    /// `[AudioVisual] EnterBioReactorSound=` is RulesClass+0x270, read by
    /// `RulesClass::ReadAudioVisual @ 0x006691E0` and played after successful
    /// InfantryAbsorb admission in `InfantryClass::Per_Cell_Process
    /// @ 0x00519630` (`0x0051A298..0x0051A30A`). `None` is the constructor's
    /// invalid Voc index; the fixed SOUNDMD binder resolves the name.
    pub enter_bio_reactor_sound: Option<String>,
    /// Rules+268: ReadAudioVisual66A10B..66A141 resolves the exact-case
    /// EnterGrinderSound through the fixed Voc catalog, retaining the prior
    /// index on absent/empty/unknown values. Constructor default is -1.
    pub enter_grinder_sound: Option<String>,
    /// `[AudioVisual] UpgradeVeteranSound=` — `RulesClass::ReadAudioVisual @
    /// 0x006691E0` resolves it to a Voc index at `RulesClass+0x228`
    /// (`0x00669CFD..0x00669D27`); `TechnoClass::AI_Update` plays that index
    /// positionally on a veteran promotion (`0x006FA124..0x006FA12A`). The
    /// fixed SOUNDMD binder retains the last valid index through reached
    /// passes; absent, empty and unknown values keep it. Constructor66596A=-1.
    pub upgrade_veteran_sound: Option<String>,
    /// `[AudioVisual] UpgradeEliteSound=` — `RulesClass+0x22C`
    /// (`0x00669D2D..0x00669D68`, constructor665970=-1), played on an elite promotion
    /// (`0x006FA0B6..0x006FA0BC`).
    pub upgrade_elite_sound: Option<String>,
    /// `[AudioVisual] SlavesFreeSound=` — `RulesClass::ReadAudioVisual @
    /// 0x006691E0` resolves it to a Voc index at `RulesClass+0x234`
    /// (constructor -1); the slave release plays it at the first freed slave
    /// (`0x006B0C2C..0x006B0C60`). Absent or empty is silence.
    pub slaves_free_sound: Option<String>,
    /// The seven `[AudioVisual]` crate pickup sounds `RulesClass::ReadAudioVisual
    /// @ 0x006691E0` resolves to Voc indexes at `RulesClass+0x1E4..+0x1FC`
    /// (`0x00669746..0x00669903`, each `ReadString` then `VocClass::FindIndex`,
    /// keeping the previous index on a miss). `CellClass::PickupCrate @
    /// 0x00481A00` plays one at the crate, for the local player's house
    /// except Reveal, which plays for everyone (`0x0048202D`):
    /// `CrateMoneySound=` (`+0x1E4`, `0x00482553`), `CrateRevealSound=`
    /// (`+0x1E8`, `0x0048202F`), `CrateFireSound=` (`+0x1EC`, `0x004832E4`),
    /// `CrateArmourSound=` (`+0x1F0`, `0x00482F22`), `CrateSpeedSound=`
    /// (`+0x1F4`, `0x00483111`), `CrateUnitSound=` (`+0x1F8`, `0x0048243E`)
    /// and `CratePromoteSound=` (`+0x1FC`, `0x00482B7B`). The name is retained
    /// at the data boundary; absent or empty is silence.
    pub crate_money_sound: Option<String>,
    pub crate_reveal_sound: Option<String>,
    pub crate_fire_sound: Option<String>,
    pub crate_armour_sound: Option<String>,
    pub crate_speed_sound: Option<String>,
    pub crate_unit_sound: Option<String>,
    pub crate_promote_sound: Option<String>,
    /// `[AudioVisual] EliteFlashTimer=` — `RulesClass+0xBE8`. Seeded into
    /// `TechnoClass+0xF0` on an elite promotion (`0x006FA0D6..0x006FA0DC`),
    /// whatever house owns the object. Constructor default UNCHECKED.
    pub elite_flash_timer: i32,
    /// Exact `[AudioVisual] IdleActionFrequency=` double, `Rules+0x1710`.
    /// Original constructor667574/66757E stores0x3FB53F7CED916873;
    /// AudioVisual66B3EA reads over its current value. Infantry51CDB0 uses
    /// this value to arm its idle timer before subsequent idle/RNG decisions.
    /// Preserve the native double; milliunit quantization changes its timer.
    pub idle_action_frequency: f64,
    /// `[AudioVisual] ForceShieldColor=`, a `[ColorAdd]` index
    /// (`Rules+0x18B0`): ReadInt at `0x0066B877..0x0066B891` over its current
    /// value, constructor 0 (`0x006678C1`). A Force Shielded building's draws
    /// OR its colour into their pixels (`app/presentation/lighting.rs`).
    pub force_shield_color: i32,
    /// AudioVisual LaserTargetColor (Rules+18A4), constructor6678AA=0;
    /// ReadInt66B818..66B832 retains the current ColorAdd table index.
    pub laser_target_color: i32,
    /// Phobos `[AudioVisual] AirstrikeLineColor=`, `RulesExt`'s
    /// `Valueable<ColorStruct>` (constructor `255,0,0`, `src/Ext/Rules/Body.h`):
    /// the RGB an airstrike beam and its target dot draw. A TechnoType's own
    /// `AirstrikeLineColor` overrides it (`Simulation::airstrike_line_color`).
    #[cfg(feature = "phobos")]
    pub airstrike_line_color: [u8; 3],
    /// Phobos `[AudioVisual] AirstrikeLineZAdjust=`, `RulesExt`'s
    /// `Valueable<int>` (constructor 0): added to the smaller of the beam's two
    /// end depths, which `TechnoClass_DrawAirstrikeFlare_LineColor`
    /// (`0x007058F6`) then gives the whole beam.
    #[cfg(feature = "phobos")]
    pub airstrike_line_z_adjust: i32,
    /// AudioVisual66B832..66B857, Rules+18A8; constructor6678B0=0.
    pub iron_curtain_color: i32,
    /// AudioVisual66B857..66B877, Rules+18AC; constructor6678B6=0.
    pub berserk_color: i32,
    /// AudioVisual66A046..66A07C / 66A088..66A0B5 use ReadString128 then
    /// VocFindIndex7514D0, retaining the current index on a miss (ctor -1).
    pub airstrike_abort_sound: Option<String>,
    pub airstrike_attack_voice: Option<String>,
    /// `[AudioVisual] GateDown`/`GateUp` (Rules+0x408/+0x404, ReadString128
    /// then VocFindIndex, constructor -1): the sounds Mission_Open plays as a
    /// gate opens (`0x0044E550`) and closes (`0x0044E61E`). Only the Ares
    /// gate-sound hooks read them, so they are bound only in Rules read the
    /// Ares way; a stock game leaves the stock sound a residual
    /// (`sim::gate_runtime`).
    pub(super) gate_sounds: [Option<String>; 2],
    /// `ConditionRedSparkingProbability=` ([General]) — per-tick probability that
    /// the `AI_Update` damage-Spark particle system spawns while health is below
    /// ConditionRed. Default **0.02** (verified `RulesClass__Constructor`; stock INI
    /// does NOT set this key). Stored as **f64** because gamemd reads it with
    /// `ReadDouble` and compares it as a double; the f32-vs-f64 rounding shifts the
    /// integer roll threshold by 1 (→ desync). Consumed via `condition_red_spark_threshold`.
    pub condition_red_sparking_probability: f64,
    /// `ConditionYellowSparkingProbability=` ([General]) — per-tick spawn probability
    /// in the yellow band (ConditionRed <= ratio < ConditionYellow). Default **0.01**
    /// (verified `RulesClass__Constructor`). f64 for the same reason as the red band.
    pub condition_yellow_sparking_probability: f64,
    /// Integer roll threshold for the red-band damage-Spark prob-roll, precomputed
    /// from `condition_red_sparking_probability` at parse time: the per-tick test
    /// becomes the pure-integer `roll < threshold` (no float in the sim hot path).
    /// `roll = scenario_rng.next_range_u32_inclusive(0, 0x7ffffffe)`. See
    /// [`unit_draw_threshold`]. Default 42_949_673 (band 0.02).
    pub condition_red_spark_threshold: u32,
    /// Integer roll threshold for the yellow-band damage-Spark prob-roll.
    /// Default 21_474_837 (band 0.01).
    pub condition_yellow_spark_threshold: u32,
    /// AI coefficient for the scorer weapon's effectiveness against a candidate.
    pub dumb_my_effectiveness_coefficient: f64,
    /// AI coefficient for the candidate weapon's effectiveness against the scorer.
    pub dumb_target_effectiveness_coefficient: f64,
    /// AI coefficient for the candidate type's `SpecialThreatValue=`.
    pub dumb_target_special_threat_coefficient: f64,
    /// AI coefficient for the candidate's live health ratio.
    pub dumb_target_strength_coefficient: f64,
    /// AI coefficient for whole cells beyond the selected weapon range.
    pub dumb_target_distance_coefficient: f64,
    /// Retained General defaults (original Rules+1040..1060,
    /// ctor666C3F..666C75 zero, ReadGeneral671AF6..671B97).
    pub(super) threat_coefficient_defaults: crate::rules::threat_coefficients::ThreatCoefficients,
    /// `EnemyHouseThreatBonus=` (`RulesClass+0x1090`, ReadDouble at
    /// `0x00671C7C`; constructor 0.0 at `0x00666CB7`, stock rulesmd 400):
    /// added to the threat score of a candidate the scorer house's current
    /// enemy owns (`TechnoClass::Calculate_Threat_Score @ 0x0070CF13`).
    pub enemy_house_threat_bonus: f64,
    /// `ThreatPerOccupant=` (`RulesClass+0x0DF4`, `RulesClass::ReadGeneral @
    /// 0x00670128`; constructor default 5 at `0x006668DE`, stock rulesmd 10).
    /// A garrisoned building's `ThreatPosed` is `occupants * this` instead of
    /// its own type value (`TechnoClass::Get_ThreatPosed @ 0x00708B40`).
    pub threat_per_occupant: i32,
    /// `NormalTargetingDelay=` ([General], stock 27; `Rules+0xE08`, ReadInt
    /// `0x006701BA..0x006701D3`, constructor 27 at `0x00666911`, no clamp) —
    /// frames between passive target scans for every mission except Area
    /// Guard. The per-object scan timer is re-armed to this value plus a
    /// 0..=2 scenario-RNG jitter.
    pub normal_targeting_delay: i32,
    /// `[General] DeadBodies=` (`Rules+0x124`): the corpse anims an
    /// infantryman's Die1..Die5 completion picks from when its type names none
    /// (`0x00520C42..0x00520C91`). Retail: `DEATH_A`..`DEATH_F`.
    pub dead_bodies: Vec<String>,
    /// `GuardAreaTargetingDelay=` ([General], stock 36; `Rules+0xE04`, ReadInt
    /// at `0x0067019A..0x006701B4`, constructor 36 at `0x00666906`, no clamp)
    /// — the same cadence for an Area Guard object, which scans twice as far
    /// and so scans less often (`0x0070985E`). It is also the dwell after a
    /// shot before a unit's idle turret returns (`0x00736B45`) and before a
    /// Gattling building's idle decay starts (`0x0043FEE9`), each plus 5.
    pub guard_area_targeting_delay: i32,
    /// SFX played when the first occupant enters a CanBeOccupied building.
    /// Parsed from [AudioVisual] BuildingGarrisonedSound (typically "BuildingGarrisoned").
    /// None = no sound configured. Resolved at app layer to a sound.ini entry.
    pub building_garrisoned_sound: Option<String>,
    /// Rules+1C0, [AudioVisual] BuildingAbandonedSound. Constructor6658D4
    /// stores -1. Reader669C23..669C62 follows BuildingGarrisonedSound and
    /// precedes BuildingRepairedSound: ReadString128, Voc Find7514D0, retain
    /// the previous ID for absent/empty/unknown names. Bound by the shared
    /// sound-reference reader over the processed rules passes.
    pub building_abandoned_sound: Option<String>,
    /// Global wall/building sale cue from `[AudioVisual] SellSound=`.
    pub sell_sound: Option<String>,
    /// Base-alert siren from `[AudioVisual] BaseUnderAttackSound=` (stock
    /// `BaseUnderAttackSiren`), stored at `Rules+0x184`.
    ///
    /// gamemd: `HouseClass::NotifyUnderAttack @ 0x004F95B8..0x004F95CF` plays
    /// it non-positionally (pan `0x2000`, volume `1.0f`, no handle) through
    /// `VocClass::PlayAtPos @ 0x00750920`, right after the EVA line at
    /// `0x004F95B3` and only when `CreateRadarEvent @ 0x0065FA70` accepted the
    /// ping. The ore-miner branch plays `EVA_OreMinerUnderAttack` at
    /// `0x004F94FB` and then jumps (`0x004F9500 JMP`) to `LAB_004F95D4`, so a
    /// harvester under attack gets the EVA and **no** siren. The
    /// `EVA_OurAllyIsUnderAttack` branch does NOT jump past — native sirens
    /// for an ally's base too, and so does VERA (`AllyUnderAttack`).
    pub base_under_attack_sound: Option<String>,
    /// Building crumble cue from `[AudioVisual] BuildingDieSound=` (stock
    /// `BuildingGenericDie`).
    ///
    /// gamemd: `RulesClass::ReadAudioVisual @ 0x0066AA49..0x0066AA88` stores
    /// the `VocClass::FindByName` result in `Rules+0x6E8`.
    /// `BuildingClass::DestructionEffects` plays it at the building's own
    /// coordinate (`0x0044174E LEA EAX,[ESI+0x9C]`, then
    /// `0x00441773 MOV ECX,[Rules+0x6E8]`, `0x00441779 CALL
    /// VocClass::PlayAtCoord @ 0x00750E20`) — but **only when the building
    /// type's own `DieSound=` list is empty**: `0x0044173F MOV
    /// ECX,[type+0x520]` is that `DynamicVectorClass`'s count
    /// (`TechnoTypeClass+0x510..0x528`) and `0x0044174A CMP ECX,EBX ; JNZ`
    /// skips the global when it is non-zero (`EBX` is zeroed for the whole
    /// function at `0x00441606`).
    pub building_die_sound: Option<String>,
    /// Struck-building cue from `[AudioVisual] BuildingDamageSound=` (stock
    /// `BuildingDamaged`), stored at `Rules+0x714`.
    /// Original gamemd ctor665650 stores -1 at666045;
    /// ReadAudioVisual6691E0's block66AC98..66ACDD passes literal83A460,
    /// empty889F64 and capacity128 to ReadString528A10, then7514D0.
    /// The fixed-catalog process binder retains earlier valid references for
    /// absent, empty or unregistered later values; no unresolved name escapes.
    ///
    /// gamemd: `BuildingClass::ReceiveDamage @ 0x00442230` plays it at the
    /// building's own coordinate (`0x004426DB LEA ECX,[ESI+0x9C]`,
    /// `0x00442700 MOV ECX,[Rules+0x714]`, `0x00442706 CALL
    /// VocClass::PlayAtCoord @ 0x00750E20`). It is **not** a per-hit cue: the
    /// shared Techno receiver's result selects the arm. `0x0044242C MOV
    /// AL,[ESI+0x90]` (`ObjectClass::IsAlive`) skips the whole dispatch for a
    /// dead building; otherwise `0x00442476 JMP [EAX*4 + 0x00442C18]` with
    /// `EAX = result - 2` enters the four-entry table
    /// `{0x004426AC, 0x004426C8, 0x004424A2, 0x0044247D}`, and only entries 0
    /// and 1 — result 2 (the hit crossed HP from `>= Strength >> 1` to below
    /// it) and result 3 (it crossed below `Strength * Rules+0x1708`,
    /// ConditionRed) — reach the gate at `0x004426D2 CMP [type+0x538],-1`.
    /// A type with its own `DamageSound=` takes `JNZ 0x0044270B` and the
    /// global is skipped.
    pub(super) building_damage_sound: Option<String>,
    /// Rules+178/+17C/+180, ctor665868..665874=-1. Original AudioVisual
    /// 66A3AD..66A472 reads these fixed-catalog scalar names in that order
    /// with128-byte retention readers;
    /// House RecordLastBuilt4FB6B0 selects the concrete class before the
    /// produced type's CreateSound override. Buildings have no class fallback.
    pub(super) create_unit_sound: Option<String>,
    pub(super) create_infantry_sound: Option<String>,
    pub(super) create_aircraft_sound: Option<String>,
    /// Rules+28C: ctor665A00=-1; AudioVisual66A355..66A38C reads the
    /// exact DiskLaserChargeUp key through ReadString128/Voc FindIndex.
    /// The fixed-catalog binder owns retention across reached rules passes.
    pub(super) disk_laser_charge_up: Option<String>,
    /// Thunder cues from `[AudioVisual] LightningSounds=` (stock a single
    /// `WeatherStrike`), in source order.
    ///
    /// gamemd: `CCINIClass::ReadSoundList @ 0x00525430` reads the value with
    /// `ReadString(..., "", buf, 0x80)`, tokenises it with `strtok` on `","`
    /// (`0x00817F70`) and keeps only names `VocClass::FindPtrByName` resolves,
    /// leaving the vector at `Rules+0x734` (items `+0x738`, count `+0x744`).
    /// `LightningStorm::GroundStrike @ 0x0053A45F..0x0053A4A2` skips the cue
    /// entirely when the count is zero and otherwise plays
    /// `items[rand % count]` at the strike coordinate.
    pub lightning_sounds: Vec<String>,
    /// Displayed-credits tick cues from `[AudioVisual] CreditTicks=` (stock
    /// `CreditUp,CreditDown`), the `Rules+0x6D0` sound list (count `+0x6DC`).
    ///
    /// gamemd: `CreditsClass::Draw @ 0x004A24F4..0x004A2533` plays
    /// `items[counting_up(+0x9) ? 0 : 1]` through `VocClass::PlayAtPos @
    /// 0x00750920` at volume `0.5f`, pan `0x2000` (centre), only while the
    /// `animating(+0xA)` latch set by `CreditsClass::AI @ 0x004A2600` on a
    /// step that changed the displayed value is up, and only when the list
    /// holds at least two entries (`0x004A2505 CMP [EAX+0x6dc],2 / JL`).
    /// Same `ReadSoundList` tokenisation as `lightning_sounds`; resolution
    /// is deferred to the app audio registry.
    pub credit_ticks: Vec<String>,
    /// Weather-storm start cue from `[AudioVisual] StormSound=` (stock
    /// `WeatherIntro`), stored at `Rules+0x730`.
    ///
    /// gamemd: `RulesClass::ReadAudioVisual @ 0x0066AEAE..0x0066AEE6` reads
    /// the key at `0x0083A400` and keeps the `VocClass::FindByName` result.
    /// `LightningStorm::Start @ 0x0053A032..0x0053A044` plays it
    /// **non-positionally** — `MOV ECX,[Rules+0x730]`, `MOV EDX,0x2000`
    /// (centred pan), `PUSH 0x3F800000` (volume `1.0f`), no handle, through
    /// `VocClass::PlayAtPos @ 0x00750920` — and only inside the
    /// `0x0053A014 MOV AL,[Rules+0x17B0]` gate it shares with the on-screen
    /// storm message. See [`GeneralRules::lightning_print_text`].
    ///
    /// It is **not** a launch cue. `SuperClass::Launch @ 0x006CC390` case 2
    /// passes `[Rules+0x1794]` (`LightningDeferment`, stock 250) as `Start`'s
    /// `param_2`, and `Start` returns at `if (param_2 != 0)` before reaching
    /// `0x0053A044`; `LightningStorm::Process @ 0x0053A6C0` re-enters with
    /// `param_2` cleared (`0x0053AAC8 XOR EDX,EDX`) once the countdown expires
    /// and that entry plays it.
    pub storm_sound: Option<String>,
    /// `[General] LightningPrintText=` (`Rules+0x17B0`), the gate on both the
    /// storm message and [`GeneralRules::storm_sound`].
    ///
    /// gamemd: `RulesClass::ReadGeneral @ 0x0067107F..0x0067108C` reads the
    /// key at `0x0083BC74`. Stock `rulesmd.ini` does **not** author it, so the
    /// constructor default decides: `RulesClass::Constructor @ 0x006676BC MOV
    /// byte ptr [ESI+0x17B0],AL`, where the last definition of `EAX` before it
    /// is `0x00667202 MOV EAX,0x1` and the function's last `CALL` is at
    /// `0x0066715D` — so the default is **true** and the storm cue does play
    /// on retail data.
    pub lightning_print_text: bool,
    /// Nuclear-missile launch cue from `[AudioVisual] DigSound=` (stock
    /// `NukeSiren`), stored at `Rules+0x174`.
    ///
    /// gamemd: `RulesClass::ReadAudioVisual @ 0x00669331..0x00669367` reads
    /// the key at `0x0083AB70` (`"DigSound"`). Despite the name it is the
    /// nuke siren — stock `rulesmd.ini` says so in a comment.
    /// `SuperClass::Launch @ 0x006CC390` case 0 (`Type=MultiMissile`) plays
    /// it at the target coordinate through `VocClass::PlayAtCoord @
    /// 0x00750E20` on both branches (`0x006CDCA8`/`0x006CDCAE` when the silo
    /// animates the launch, `0x006CDDE3`/`0x006CDDE9` when it does not), and
    /// the Tunnel locomotor plays it at the digger as it digs in and out
    /// (`movement::tunnel_movement`).
    pub dig_sound: Option<String>,
    /// `[AudioVisual] Dig=` (`Rules+0x294`), the AnimType the Tunnel
    /// locomotor spawns at the digger as it digs in and out: ReadString and
    /// the AnimType lookup at `0x0066A587..0x0066A5B1` over the constructor's
    /// null (`0x00665A10`). Retail leaves it unset.
    pub dig_anim: Option<String>,
    /// `[General] NukeTakeOff=` (stock `NUKETO`), the AnimType at `Rules+0x98`
    /// (ReadString then the AnimType lookup, `0x0066D829..0x0066D850`;
    /// constructor null at `0x00665735`). `BuildingClass::Mission_Missile`
    /// plays it at the silo's launch point (`0x0044CC62`).
    pub nuke_take_off: String,
    /// `[General] AISuperDefenseProbability=` (`Rules+0xEC4`, the IntVector
    /// reader at `0x0067038E`; constructor empty), indexed by a house's
    /// difficulty: the percent chance a computer house takes up the defence
    /// when an `AIDefendAgainst=` weapon targets near its base.
    pub ai_super_defense_probability: Vec<i32>,
    /// `[General] AISuperDefenseDistance=` (`Rules+0xEE4`, ReadRange at
    /// `0x006703D9`, leptons; constructor 10 at `0x00666A35`): the alert's
    /// reach from the house's base centre.
    pub ai_super_defense_distance: i32,
    /// `[General] AISuperDefenseFrames=` (`Rules+0xEE0`, ReadInt at
    /// `0x006703A4..0x006703BE` with the field as its default; constructor
    /// 25 at `0x00666A24`): how long the launch alert lasts. A computer house
    /// aims its Force Shield at the alerted cell while the alert is younger
    /// (`HouseClass::AI_TryFireSW`, `0x00509A7F..0x00509A99`).
    pub ai_super_defense_frames: i32,
    /// `[General] AIIonCannon*Value=`: what a computer house values an enemy
    /// object by when it aims a superweapon.
    pub ai_ion_cannon_values: AiIonCannonValues,
    /// `[AudioVisual] PsychicDominatorActivateSound=` (stock
    /// `PsychicDominatorActivate`), stored at `Rules+0x24C`.
    ///
    /// gamemd: `SuperClass::Launch @ 0x006CC390` case 7 plays it at the target
    /// coordinate — `0x006CCE22 MOV ECX,[Rules+0x24C]`,
    /// `0x006CCE28 CALL VocClass::PlayAtCoord @ 0x00750E20`.
    pub psychic_dominator_activate_sound: Option<String>,
    /// `[AudioVisual] GeneticMutatorActivateSound=` (stock
    /// `GeneticMutatorActivate`), stored at `Rules+0x250`.
    ///
    /// gamemd: `SuperClass::Launch @ 0x006CC390` case 9 plays it at the target
    /// coordinate — `0x006CD8CD MOV ECX,[Rules+0x250]`,
    /// `0x006CD8D3 CALL VocClass::PlayAtCoord @ 0x00750E20`.
    pub genetic_mutator_activate_sound: Option<String>,
    /// `[AudioVisual] PsychicRevealActivateSound=` (stock
    /// `PsychicRevealActivate`), stored at `Rules+0x254`.
    ///
    /// gamemd: `SuperClass::Launch @ 0x006CC390` case 11 plays it at the
    /// target coordinate — `0x006CD7B9 MOV ECX,[Rules+0x254]`,
    /// `0x006CD7BF CALL VocClass::PlayAtCoord @ 0x00750E20`. This case plays
    /// no EVA line.
    pub psychic_reveal_activate_sound: Option<String>,
    /// `[AudioVisual] SpyPlaneCamera=` (stock `SpyPlaneSnapshot`), the sound
    /// index at `Rules+0x280` (constructor -1, no sound, at `0x006659EE`):
    /// ReadString128 then `VocClass::FindIndex @ 0x007514D0`, keeping the
    /// prior index for a missing, empty or unknown name
    /// (`0x0066A295..0x0066A2C2`); [`RuleSet::bind_type_sound_references`]
    /// resolves it. `AircraftClass::Mission_SpyplaneApproach @ 0x004155F0`
    /// plays it at the plane through `VocClass::PlayAt @ 0x007509E0`
    /// (`0x004156FB`) on each camera snapshot.
    pub spy_plane_camera: Option<String>,
    /// `[AudioVisual] SpyPlaneCameraFrames=` (`Rules+0x290`, ReadInteger at
    /// `0x0066A39A` with the field as its default; constructor 16 at
    /// `0x00665A06`): the frames Mission_SpyplaneApproach returns, its
    /// mission timer (`0x0041578D`, `0x004157A0`, `0x004157B3`).
    pub spy_plane_camera_frames: i32,
    /// SFX played when a paradropped passenger successfully deploys a parachute.
    /// Parsed from [AudioVisual] ChuteSound (stock "ParachuteDrop").
    /// None = no sound configured. Resolved at app layer to a sound.ini entry.
    pub chute_sound: Option<String>,
    /// Sound event for shell main-menu buttons from [AudioVisual] GUIMainButtonSound.
    pub gui_main_button_sound: Option<String>,
    /// Shell first-paint controls-reveal slide-in cue from [AudioVisual]
    /// GUIMoveInSound (stock `MenuSlideIn`). Played once at the start of each
    /// allow-listed shell dialog's slide. None = no sound configured.
    pub gui_move_in_sound: Option<String>,
    /// Shell teardown slide-out cue from [AudioVisual] GUIMoveOutSound (stock
    /// `MenuSlideOut`, `RulesClass+0x19C` read at `0x006694C7`), played by
    /// `0x00608070` before a shown shell dialog's buttons slide out.
    pub gui_move_out_sound: Option<String>,
    /// Shell slide-in end cue from [AudioVisual] ShellButtonSlideSound
    /// (`RulesClass+0x750`, constructor -1 at `0x00666098`). `0x0066AF8D`
    /// reads 128 bytes and keeps the prior value on an empty copy or an
    /// unknown name (`0x007514D0` -1); like its siblings, the name resolves
    /// only when played. The slide engine `0x006071E0` plays it as each
    /// slide-in ends (`0x00607F59`). Stock rules leave it empty.
    pub shell_button_slide_sound: Option<String>,
    /// Generic click from [AudioVisual] GenericClick (`Rules+0x70C`,
    /// constructor `0x00666039` = -1). AudioVisual `0x0066AD24..0x0066AD56`
    /// uses ReadString128/Voc FindIndex7514D0; failed reads keep the prior ID.
    /// The fixed SOUNDMD binder resolves it. A human player's sale order plays it too
    /// (`Sell_Back @ 0x00447110`), and so does switching a repair off, or on
    /// at health unequal to Strength (`BuildingClass::ToggleRepair @ 0x00446FF0`).
    pub generic_click_sound: Option<String>,
    /// Guard command acknowledgement, Rules+724. Constructor665650 starts
    /// at -1; AudioVisual66AE1C..66AE67 reads GuardSound with ReadString128
    /// then Voc FindIndex7514D0, retaining the prior ID on missing, empty or
    /// unresolved text. The fixed SOUNDMD binder owns name resolution.
    pub guard_sound: Option<String>,
    /// Stop command acknowledgement, Rules+720, read just before GuardSound
    /// by the same reader (`0x0066ADE3..0x0066AE16`).
    pub stop_sound: Option<String>,
    /// Rules+6F0/+6F4: constructor66600F/666015 starts at -1;
    /// AudioVisual66AAC0..66AB53 reads exact RadarOn then RadarOff through
    /// ReadString128/Voc7514D0, retaining the prior fixed-catalog ID on
    /// missing, empty or unknown names. Radar656BE0 requests them centered,
    /// full-volume and without a retained handle on ordinary direction changes.
    /// Source: tools/input_oracle/radar_availability_audio.md and its consumed corpus.
    pub radar_on: Option<String>,
    pub radar_off: Option<String>,
    /// Rules+6F8/+6FC: the radar transmission's static, read next as exact
    /// MovieOn then MovieOff by the same reader (`0x0066AB5F..0x0066ABC7`).
    /// `RadarClass::PlayRadarMovie @ 0x006578F0` and `PerFrameMovieUpdate @
    /// 0x006579E0` play them centred at full volume on the radar's handle.
    pub movie_on: Option<String>,
    pub movie_off: Option<String>,
    /// Rules+220/+224: constructor66595E/665964 starts at -1;
    /// AudioVisual66A4EF..66A582 reads SpySatActivationSound then
    /// SpySatDeactivationSound with ReadString128/Voc7514D0 and retains the
    /// current fixed-catalog ID on missing, empty or unknown names.
    /// House508F60 requests the local cue after its map callback/latch edge.
    /// Source: tools/input_oracle/spy_sat_audio.md and its consumed corpus.
    pub spy_sat_activation_sound: Option<String>,
    pub spy_sat_deactivation_sound: Option<String>,
    /// Rules+230, VoiceIFVRepair. Ctor665976 starts at -1;
    /// AudioVisual669D63..669DA7 uses ReadString128/Voc7514D0, retaining
    /// the current ID on missing/empty/unknown text. Fixed SOUNDMD binding
    /// supplies the name. Native controls: spatial_oracle/attack_order_voice.
    pub(super) voice_ifv_repair: Option<String>,
    /// Rules+1D4: constructor6658F2=-1; AudioVisual6696C0..6696FF reads
    /// StartPlanningModeSound with ReadString128 and Voc Find7514D0,
    /// retaining the prior ID on a missing or invalid name.
    pub start_planning_mode_sound: Option<String>,
    /// Rules+1E0: constructor665904=-1; AudioVisual66970A..669740 reads
    /// EndPlanningModeSound immediately after Start, with the same reader.
    pub end_planning_mode_sound: Option<String>,
    /// Rules+1D8: constructor6658F8=-1; AudioVisual669B25..669B52 reads
    /// AddPlanningModeCommandSound later in the same pass, retaining the ID.
    pub add_planning_mode_command_sound: Option<String>,
    /// Rules+1CC: constructor6658E6=-1; AudioVisual669BA8..669BD3 reads
    /// exact PlaceBeaconSound with ReadString128 and Voc Find7514D0,
    /// retaining the prior ID on a missing or unknown name. PlaceBeacon
    /// 430D7D..430D94 plays it centred at full volume for the local House.
    pub place_beacon_sound: Option<String>,
    /// ScatterSelectedObjects730FE0's centered cue. Rules+728 starts at -1
    /// (66585F/666063); AudioVisual66AE6C..66AEA2 reads exact ScatterSound
    /// with ReadString128 and fixed Voc lookup, retaining failed selections.
    pub scatter_sound: Option<String>,
    /// `[AudioVisual] ScoldSound=` (`Rules+0x700`, read at `0x0066ABE8`
    /// through `VocClass::FindByName`; retail `MenuScold`): ToggleRepair's
    /// sound when a repair is switched on at full Strength (`0x00447068`),
    /// and PlanningMode refusal63A0D0. Constructor666027 stores -1;
    ///66ABD3..66AC12 retains the prior resolved ID on failed/empty reads.
    pub scold_sound: Option<String>,
    /// Launcher Options Sound/Voice preview cue from [AudioVisual] GenericBeep.
    pub generic_beep_sound: Option<String>,
    /// Sound event for shell checkboxes from [AudioVisual] GUICheckboxSound.
    pub gui_checkbox_sound: Option<String>,
    /// `[General] OreTwinkle=` AnimType name; `None` keeps the constructor's
    /// null pointer so the post-load twinkle pass is skipped.
    ///
    /// gamemd: `RulesClass::ReadGeneral` at `0x0066D661..0x0066D699` reads
    /// section "General" (`0x00826278`) key "OreTwinkle" (`0x0083CF4C`) with an
    /// empty default and capacity 0x80, then resolves a non-empty value through
    /// `AnimTypeClass::Find_Or_Allocate @ 0x00428B80` into `Rules+0x1870`.
    pub ore_twinkle: Option<String>,
    /// `[AudioVisual] OreTwinkleChance=`: one twinkle roll in N per resource cell.
    ///
    /// gamemd: `RulesClass::ReadAudioVisual` at `0x0066B7F8..0x0066B812` reads
    /// section "AudioVisual" (`0x00839EA8`) key "OreTwinkleChance"
    /// (`0x0083A1CC`) through `INIClass::ReadInt @ 0x005276D0` into
    /// `Rules+0x186C`; the constructor default is 0x32.
    pub ore_twinkle_chance: i32,
    /// `[AudioVisual] GUIBuildSound=` (`Rules+0x18C`, read at `0x006693C1`
    /// through `VocClass::FindByName`; retail `MenuClick`): the sidebar cameo
    /// click sound, played by `SelectClass::Action @ 0x006AAD00` for every
    /// click that acts (e.g. `0x006AAE2A`, `0x006AB713`). The retained
    /// GlobalSoundReferences owner resolves against the fixed Voc catalog.
    pub gui_build_sound: Option<String>,
    /// `[AudioVisual] BuildingSlam=` (`Rules+0x6EC`, read at `0x0066AAA7`
    /// through `VocClass::FindByName`; retail `PlaceBuilding`): the PLACE
    /// event handler `HouseClass @ 0x004FB0E0` plays it centred at full
    /// volume (`0x004FB2FD..0x004FB314`) for the player's own house after the
    /// building's Unlimbo succeeds. GlobalSoundReferences retains its fixed
    /// Voc binding; constructor666009 starts at -1.
    pub building_slam: Option<String>,
    /// Sidebar tab click sound from [AudioVisual] GUITabSound (retail
    /// `MenuTab`; `Rules+0x190`, read at `0x00669402..0x0066942C`).
    /// `SidebarClass::Action 0x006A7780` plays it (`VocClass::PlayAt
    /// 0x00750920`, volume 1.0) at `0x006A7A66` for the tab inputs
    /// `0x80CB..0x80CE`, including a click on the tab that is already active
    /// (`0x006A7928` jumps straight to it).
    pub gui_tab_sound: Option<String>,
    /// Rules+6AC, ctor665FB0=-1, reader66A7B3..66A7E9. Fixed SOUNDMD
    /// reference; missing/unknown later names retain the previous valid ID.
    pub(super) incoming_message_sound: Option<String>,
    /// Rules+6A8, ctor665FAA=-1, AudioVisual66A763..66A7A8. Exact
    /// GameClosed ReadString128/VocFind7514D0 precedes IncomingMessage;
    /// empty/unknown later values retain the prior fixed SOUNDMD reference.
    /// LAN discovery open→closed5E02C6..5E031E plays this before its chat.
    pub(super) game_closed_sound: Option<String>,
    /// Rules+6C4, ctor665FD5=-1, reader66A7EF..66A822. Exact
    /// [AudioVisual] MessageCharTyped via ReadString128/VocFindIndex7514D0;
    /// drawn nonsilent TextLabels request it at72A609, pan2000/volume1.
    pub(super) message_char_typed_sound: Option<String>,
    /// Rules+6B0, constructor665FB6=-1. AudioVisual66A828..66A86D
    /// reads exact SystemError through ReadString128/VocFind7514D0,
    /// retaining the prior fixed-SOUNDMD ID on missing/empty/unknown text.
    /// LAN join validation5DF410 and timeout5DC506 play it centered at1.0.
    pub(super) system_error_sound: Option<String>,
    /// Rules+6B4, constructor665FBC=-1. AudioVisual66A868..66A8AE reads exact
    /// OptionsChanged after SystemError through ReadString128/VocFind7514D0,
    /// retaining the prior ID on missing/empty/unknown text. A main-menu typed
    /// code (0x00531ECB..0x00531EDB) plays it centred at volume 1.0; the
    /// serial/modem setup dialogs (0x005B5E71, 0x005B962C; game modes 1/2,
    /// which VERA lacks) are its only other players.
    pub(super) options_changed_sound: Option<String>,
    /// Rules+6BC: constructor665FC8=-1; AudioVisual66A8ED..66A932 reads
    /// exact PlayerLeft after GameForming and before PlayerJoined. ReadString
    /// capacity128/defaultempty plus fixed SOUNDMD VocFind7514D0 retains the
    /// previous ID on missing/empty/unknown text across reached Rules layers.
    /// LAN type10 endpoint removal5E10F2..5E1114 plays it only in admitted
    /// membership states>=2, independent of remaining participant count.
    pub(super) player_left_sound: Option<String>,
    /// Rules+6B8/+6C0, ctor665FC2/665FCE=-1. AudioVisual readers
    ///66A8B4..66A8E7 (GameForming) and66A938..66A973 (PlayerJoined)
    /// use ReadString128/VocFind7514D0 with prior-reference retention.
    /// Native order: OptionsChanged, GameForming, PlayerLeft, PlayerJoined.
    /// Forming is received discovery5E0280/5E0483, not own New; Joined is
    /// admitted type3 at5E0743..0770 or host type6 afteraccept7 at5E1B32.
    pub(super) game_forming_sound: Option<String>,
    pub(super) player_joined_sound: Option<String>,
    /// `[AudioVisual] MessageDelay=` native ReadDouble in6691E0 retains
    /// Rules+14C0. Constructor665650 initializes binary64 .6. Action11
    /// 6DE0EF..6DE100 converts MessageDelay*900 through the native ftol into
    /// MessageList's 16ms buckets, independently of simulation pacing.
    pub message_delay_minutes: crate::util::native_x87::NativeF64Bits,
    /// RulesClass::ReadAudioVisual6691E0,66A679..66A693: current-default
    /// `[AudioVisual] ShroudGrow`; constructor667775 initializes false.
    pub shroud_grow: bool,
    /// Rules+1640, ReadAudioVisual66B49E..66B4BE: current-default
    /// ShroudRate in minutes, f32 widened; constructor6673FC/667402 stores4.
    pub shroud_rate: NativeF64Bits,
    /// `[AudioVisual] FogRate=` in minutes, Rules+1648. Constructor
    /// 66740C/667416 stores binary64 .05; ReadAudioVisual66B4CA..66B4E4
    /// reads the current default through ReadDouble5283D0 (f32 widened).
    pub fog_rate: NativeF64Bits,
    /// `[AudioVisual] SpeakDelay=` in MINUTES (retail `2`): the EVA advice
    /// repeat interval. `RulesClass::ReadAudioVisual` reads key
    /// `"SpeakDelay"` (`0x0083A270`) through `INIClass::ReadDouble @
    /// 0x005283D0` into `Rules+0x16A8` (`0x0066B646 FSTP double`);
    /// `RulesClass::Constructor 0x006674BA` zero-fills it, so a missing key is
    /// `0.0`. `HouseClass::Update` re-arms its funds/low-power timers to
    /// `SpeedNormalize(ftol(SpeakDelay * 900.0))` (`0x004F8BB4..0x004F8BCB`,
    /// `0x007E27F8` = `900.0`).
    pub speak_delay_minutes: f64,
    /// Sound event for opening shell combo boxes from [AudioVisual] GUIComboOpenSound.
    pub gui_combo_open_sound: Option<String>,
    /// Sound event for closing shell combo boxes from [AudioVisual] GUIComboCloseSound.
    pub gui_combo_close_sound: Option<String>,
    /// Rules+194/+198: constructors665892/665898 store-1. ReadAudioVisual
    /// 669423..6694B6 reads exact GUIOpenSound then GUICloseSound through
    /// ReadString128/VocFind7514D0, retaining each prior valid ID. The fixed
    /// SOUNDMD binder owns these references, separately from the combo keys.
    /// SidebarInput6D091A/6D0860 consumes them; see tools/audio_catalog_owner.md.
    pub(super) gui_open_sound: Option<String>,
    pub(super) gui_close_sound: Option<String>,
    /// Sound used by conditional reciprocal-link harvester release. Parsed
    /// from [AudioVisual] BunkerWallsDownSound (retail value "TankBunkerDown").
    /// Stock zero-link refinery unload completion does not play it. None =
    /// no sound configured.
    pub bunker_walls_down_sound: Option<String>,
    /// Tank-bunker walls-up SFX. Parsed from [AudioVisual] BunkerWallsUpSound
    /// (retail value "TankBunkerUp"). None = no sound configured.
    pub bunker_walls_up_sound: Option<String>,
    /// Direct rocker force coefficient (DirectRockingCoefficient= in [AudioVisual]).
    /// Multiplies the final DirectRocker impulse force. Default 1.5.
    pub direct_rocking_coefficient: SimFixed,
    /// Damping coefficient applied while a vehicle is moving (FallBackCoefficient=
    /// in [AudioVisual]). Multiplies the base 0.002 rad/tick decay rate; smaller
    /// values keep the body tilted longer between successive impulses. Default 0.1.
    pub fallback_coefficient: SimFixed,
    /// Fallback sound played at the arrival cell of a self-teleport when the
    /// per-unit `ChronoInSound=` is unset. Parsed from `[AudioVisual]
    /// ChronoInSound=` (stock ships `ChronoMinerTeleport`). A genuinely-absent
    /// key yields `None` = no sound, not a fabricated fallback.
    pub chrono_in_sound: Option<String>,
    /// Fallback sound played at the departure cell of a self-teleport when the
    /// per-unit `ChronoOutSound=` is unset. Parsed from `[AudioVisual]
    /// ChronoOutSound=` (stock ships `ChronoMinerTeleport`). A genuinely-absent
    /// key yields `None` = no sound, not a fabricated fallback.
    pub chrono_out_sound: Option<String>,
    /// Rules+284/+288: ReadAudioVisual6691E0 reads the exact-case
    /// LetsDoTheTimeWarpOutAgain/InAgain keys at66A2D7..66A34F through
    /// ReadString128 then VocFind7514D0, retaining the previous valid ID.
    /// Constructor6659F4/FA stores -1. Use the fixed SOUNDMD binder.
    pub(super) time_warp_out_sound: Option<String>,
    pub(super) time_warp_in_sound: Option<String>,
    /// Rules+1C8: ctor6658E0 stores-1; ReadAudioVisual669645..66967B
    /// reads exact CheerSound through ReadString128/VocFind7514D0 with
    /// prior-ID retention. The fixed SOUNDMD binder owns name resolution.
    pub(super) cheer_sound: Option<String>,
    /// General.MaximumCheerRate: ctor665D92=1, ReadGeneral66EBD4 signed/current-default.
    pub(super) maximum_cheer_rate: i32,
    /// `[AudioVisual] ImpactWaterSound=` / `ImpactLandSound=`
    /// (`RulesClass+0x200` / `+0x204`, `RulesClass::ReadAudioVisual`
    /// `0x00669924` / `0x00669965`): a crash impact's fallback cue when the
    /// type names none (`FlyLocomotionClass::Process 0x004CD83E`/`0x004CD85C`).
    /// Stock: `ExplosionWaterLarge` and empty (silence).
    pub impact_water_sound: Option<String>,
    pub impact_land_sound: Option<String>,
    /// AudioVisual/SinkingSound -> Rules+208 (6699C8); constructor665940
    /// stores -1. Used only when the sinking type has no resolved sound.
    pub sinking_sound: Option<String>,
    /// `[AudioVisual] ScoreAnimSound=` (retail `ScoreEmblemSoundLoop`): the
    /// looping cue `Dialog0x103__DlgProc` starts when the slide-in completes
    /// (`0x006C93A0..0x006C93D4`, `VocClass__PlayGlobal` with the Rules ID),
    /// stopped by Continue and by dialog destruction. None = no cue.
    pub score_anim_sound: Option<String>,
    /// Rules+6C8 ctor665FE2=-1; AudioVisual Construction at66A97F..66A9B7
    /// uses ReadString128/Find7514D0 and keeps the prior index on invalid input.
    pub construction_sound: Option<String>,
    /// Rules+1C4 ctor6658DA=-1; AudioVisual BuildingRepairedSound at
    /// 669C68..669CA3 uses ReadString128/Find7514D0 with prior-ID retention.
    /// Techno701494 plays it after an Engineer restores a building's health.
    pub building_repaired_sound: Option<String>,
    /// `[AudioVisual] BombTickingSound=` (`RulesClass+0x20C`): the looping
    /// tick at a bombed object (`BombListClass::UpdateAll @ 0x00438BF0`).
    pub bomb_ticking_sound: Option<String>,
    /// `[AudioVisual] BombAttachSound=` (`RulesClass+0x210`): played at the
    /// bombed object for the planter's own player (`BombListClass::Attach @
    /// 0x00438FD7`).
    pub bomb_attach_sound: Option<String>,
    /// Interval in minutes between low-power degradation damage ticks on Powered=yes buildings.
    /// Parsed from DamageDelay= in [General]. Default 1.0 minute.
    pub damage_delay_minutes: f32,
    /// `[General] SpyPowerBlackout=` (`RulesClass+0xD64`, ReadInt
    /// `0x0066FBF1..0x0066FC0B`; the constructor's 0, `0x0066680A`): the
    /// frames the owner of an infiltrated power plant blacks out for
    /// (`BuildingClass::Infiltrate`, `0x004572DB`). Retail sets 1000.
    pub spy_power_blackout: i32,
    /// `[General] SpyMoneyStealPercent=` (`RulesClass+0xD68`, ReadDouble
    /// stored as a float, `0x0066FC17..0x0066FC30`; the constructor's
    /// `0x3F000000`, 0.5, `0x00666810`): the share of its owner's money an
    /// infiltrated `Storage=` building yields (`0x0045743F`). Retail sets .5.
    pub spy_money_steal_percent: NativeF32Bits,
    /// Fire/smoke anim types spawned on buildings below ConditionYellow health.
    /// Parsed from DamageFireTypes= in [General]. Default: FIRE01,FIRE02,FIRE03.
    pub damage_fire_types: Vec<AnimRef>,
    /// ReadAudioVisual66B0E4..66B1CF retains the ordered OnFire Anim pointers
    /// for BuildingReceiveDamage442785. RulesCtor666568..66659E starts empty;
    /// these runtime keys preserve distinct Anim slots with equal stored IDs.
    pub on_fire: Vec<String>,
    /// Particle system spawned by exploding barrels.
    /// Parsed from `BarrelParticle=` in `[General]` (NOT `[AudioVisual]`,
    /// despite the proximity to other AudioVisual keys).
    /// Holds the unresolved section name; ID resolution against the
    /// particle-system registry is deferred (matches A2/A3/A4/A5a pattern).
    pub barrel_particle: Option<String>,
    /// Rules+54, ctor6656D1 null; General66D6A8..66D6D5 retains the
    /// Anim factory result on missing/empty reads. AreaDamage48A34D.
    pub barrel_explode: Option<String>,
    /// Rules+58, ctor6656C0 empty; General66D6E2..66D7B1 preserves
    /// ordered Voxel factory results. AreaDamage stops at its first passing trial.
    pub barrel_debris: Vec<String>,

    // -- Harvester scan radii and economy --
    /// `Rules+0x1778`, `[General] TiberiumShortScan=` in leptons: the ring
    /// bound of Mission_Harvest state 1's next-cell scan and its full-cargo
    /// archive scan (`0x0073E9F1`, `0x0073EAA6`, `>> 8` to cells). Read by
    /// `RulesClass::ReadGeneral` at `0x0067028C..0x006702A6` through
    /// `CCINIClass::ReadRange 0x00474620` (cells x 256, chopped) over the
    /// current value; the constructor writes `0x600` (`0x00667638`). Retail
    /// `6` is 1536.
    pub tiberium_short_scan: i32,
    /// `Rules+0x177C`, `[General] TiberiumLongScan=` in leptons: the ring
    /// bound of Mission_Harvest state 0's ore search (`0x0073E851`). Read
    /// like TiberiumShortScan (`0x006702AC..0x006702C5`); the constructor
    /// writes `0x2000` (`0x00667642`). Retail `48` is 12288.
    pub tiberium_long_scan: i32,
    /// `Rules+0x1518`, `[General] BuildupTime=` in minutes: how long a
    /// building's build-up (and pack-up) animation runs, spread over its
    /// Buildup SHP's frames (`rules::buildup_asset_catalog`).
    /// `RulesClass::ReadGeneral` reads it through `CCINIClass::ReadDouble
    /// 0x005283D0` at `0x00670CB5`; the constructor writes `.05`
    /// (`0x3FA999999999999A`, `0x006673B3..0x006673BD`). Retail `.06`.
    pub buildup_time: f64,
    /// `Rules+0x1780`, `[General] SlaveMinerShortScan=` in leptons: a deployed
    /// slave refinery with ore this close keeps working where it stands.
    /// `RulesClass::ReadGeneral` reads it through `CCINIClass::ReadRange
    /// 0x00474620` at `0x006702E0`; the constructor writes `0x500`
    /// (`0x0066764C`). Retail `8` is 2048.
    pub slave_miner_short_scan: i32,
    /// `Rules+0x1784`, `[General] SlaveMinerSlaveScan=` in leptons: how far a
    /// slave looks for ore (SlaveManagerClass AI_Update state 1,
    /// `0x006AF748`). ReadRange at `0x00670300`; constructor `0x1000`
    /// (`0x00667656`). Retail `14` is 3584.
    pub slave_miner_slave_scan: i32,
    /// `[CombatDamage] DrainMoneyFrameDelay=` (`Rules+0x314`, stock 30).
    /// RulesClass::Constructor665650 writes100 at665AC8; ReadCombatDamage
    /// 66C973..66C98C retains the current signed value through ReadInt5276D0.
    /// The drained
    /// object's `TechnoClass::AI_Update @ 0x006FA167..0x006FA17B` transfers
    /// money only on frames where `g_CurrentFrameCounter % delay == 0`
    /// (signed `IDIV`, remainder test).
    pub drain_money_frame_delay: i32,
    /// `[CombatDamage] DrainMoneyAmount=` (`Rules+0x318`, stock 30).
    /// Constructor665AD2 writes0; ReadCombatDamage66C992..66C9AC retains it.
    /// Per transfer
    /// `min(amount, Available_Money(victim))` moves from the victim's wallet
    /// (`Spend_Money @ 0x004F9790`) to the drainer's (`Add_Credits @
    /// 0x004F9950`) at `0x006FA183..0x006FA1C0`.
    pub drain_money_amount: i32,
    /// `Rules+0x1788`, `[General] SlaveMinerLongScan=` in leptons: how far a
    /// slave miner looks for a field. ReadRange at `0x0067031F`; constructor
    /// `0x5000` (`0x00667660`). Retail `48` is 12288.
    pub slave_miner_long_scan: i32,
    /// `Rules+0x178C`, `[General] SlaveMinerScanCorrection=` in leptons: how
    /// much closer to ore a new spot must be before a deployed slave miner
    /// moves. ReadRange at `0x0067033F`; constructor `0x300` (`0x0066766A`).
    pub slave_miner_scan_correction: i32,
    /// `Rules+0x1790`, `[General] SlaveMinerKickFrameDelay=`: frames an idle
    /// slave miner waits before it looks for a field again. ReadInt at
    /// `0x0067035F`; constructor `0x7FFFFFFF` (`0x00667674`). Retail 150.
    pub slave_miner_kick_frame_delay: i32,
    /// `Rules+0xF0`, `[General] MaximumQueuedObjects=`: how many builds a
    /// factory's queue holds behind its active one; `FactoryClass::
    /// StartProduction` refuses an append at this count (`0x004C9CDE`).
    /// `RulesClass::ReadGeneral` reads it through `CCINIClass::ReadInt
    /// 0x005276D0` at `0x00671DA7`; the constructor writes 5 (`0x006656EE` →
    /// `0x006657CD`).
    pub maximum_queued_objects: i32,
    /// `[General] BuildingProductionQueue=` (default false), Phobos' key: a
    /// BuildingType PRODUCE queues behind a running or completed building
    /// instead of being dropped or abandoning the held product. The next
    /// building starts only after the held one is placed. Read in every
    /// build, outside `phobos_read`: VERA's own Building Production Queue
    /// option sets it through the scenario root (`PersonalRules`), and that
    /// must not switch the rest of Phobos on.
    pub building_production_queue: bool,
    /// Phobos `[General] BuildingTypeSelectable=` (default false): type select
    /// admits every building, not only the `UndeploysInto=` ones the retail ID
    /// match admits (`0x00732C61..0x00732C83`). Phobos disables the switch
    /// without Ares (`BuildingClass_Is1x1AndUndeployable_BuildingMassSelectable`).
    #[cfg(feature = "phobos")]
    pub building_type_selectable: bool,
    /// Phobos `[General] AIFireSale=` (default true): whether a computer
    /// house on its last legs (`0x004FD879..0x004FD904`, no live
    /// `Factory=` building) sells every building.
    pub ai_fire_sale: bool,
    /// Phobos `[General] AIFireSaleDelay=` in frames (default 0): the first
    /// last-legs tick starts this timer instead of selling; the sale waits
    /// until it completes. Non-positive sells at once.
    pub ai_fire_sale_delay: i32,
    /// Phobos `[General] AIAllToHunt=` (default true): whether the same
    /// last-legs block sends every techno to hunt.
    pub ai_all_to_hunt: bool,
    /// Phobos `[General] GatherWhenMCVDeploy=` (default true): whether a
    /// computer house's yard deploy runs
    /// `HouseClass::MoveGroundForcesToBase @ 0x0050C920` (`0x00739926`).
    pub gather_when_mcv_deploy: bool,
    /// Phobos `[AudioVisual] TimerBlinkColorScheme=`: the runtime ColorScheme
    /// a finished superweapon timer blinks in, `[Colors]` entry `R / 2`, or -1
    /// for `none`. Constructor 5, the third `[Colors]` entry the game uses.
    #[cfg(feature = "phobos")]
    pub timer_blink_color_scheme: i32,
    /// `Rules+0xD78`, `[General] HarvesterTooFarDistance=` in cells: a
    /// refinery farther than this is approached before the dock is reserved.
    /// `RulesClass::ReadGeneral` reads it through `CCINIClass::ReadInt
    /// 0x005276D0` at `0x0066FFEB`; the constructor writes 5 (`0x00666835`).
    pub harvester_too_far_distance: i32,
    /// `Rules+0xDF8`, `[General] ApproachTargetResetMultiplier=`: read through
    /// `INIClass::ReadInt 0x005276D0` at `0x00670150` (retail `1.5` is 1);
    /// constructor 1. A returning slave re-paths when its NavCom has drifted
    /// more than this many cells from the drop cell (SlaveManagerClass
    /// AI_Update state 4, `0x006AFB13`).
    pub approach_target_reset_multiplier: i32,
    /// `Rules+0xD7C`, `[General] ChronoHarvTooFarDistance=` in cells, the
    /// Chrono Miner's threshold. Read through `ReadInt` at `0x0067000B`; the
    /// constructor writes 50 (`0x00666846`).
    pub chrono_harv_too_far_distance: i32,

    // -- Harvester timing --
    /// `Rules+0x1520`, `[General] HarvesterLoadRate=`: the StageClass rate
    /// Mission_Harvest state 1 and Harvest_Ore_Tick arm (`0x0073E951`,
    /// `0x0073D5D1`); state 1 cuts when the stage reaches 9. Read by
    /// `RulesClass::ReadGeneral` at `0x00670CE7..0x00670D01` through
    /// `CCINIClass::ReadInt 0x005276D0`; the constructor writes 2
    /// (`0x006671CD`, `0x006673C7`). Retail leaves it unset.
    pub harvester_load_rate: i32,
    /// Whole-frame dump gate for refinery unloading (HarvesterDumpRate=).
    /// The unload accumulator advances one whole frame per unloading tick and a
    /// slot drains once it reaches this threshold; gamemd's gate is the full
    /// `HarvesterDumpRate(double) × 900 <= accumulator`. Because the accumulator
    /// is integer-stepped, the first crossing is exactly `ceil(rate × 900)`, so
    /// storing the ceiling (not a tenths-quantized value) reproduces gamemd's
    /// crossing bit-for-bit with no float in the sim gate.
    /// Default 15 (from ceil(0.016 × 900) = ceil(14.4) = 15 frames per gate):
    /// the RulesClass constructor stores the double 0.016 at Rules+0x1528
    /// (`0x006673CD..0x006673DC`) and ReadDouble at `0x00670CD4` keeps it when
    /// the key is absent, as it is in retail RULESMD.INI.
    pub harvester_dump_frames: u16,

    // -- Chrono warp delay constants --
    /// Post-warp lock duration in game frames (ChronoDelay= in [General]).
    /// Applied after Chronosphere warp. Default 60 frames.
    pub chrono_delay: i32,
    /// Chrono reinforcement warp delay in game frames (ChronoReinfDelay= in [General]).
    /// Default 180 frames.
    pub chrono_reinf_delay: i32,
    /// Distance divisor for warp delay: delay = distance_leptons / factor
    /// (ChronoDistanceFactor= in [General]). Default 48.
    pub chrono_distance_factor: i32,
    /// Whether warp delay scales with distance (ChronoTrigger= in [General]).
    /// If false, always use ChronoMinimumDelay. Default true.
    pub chrono_trigger: bool,
    /// Minimum warp delay in game frames (ChronoMinimumDelay= in [General]).
    /// Floor for the distance-based calculation. Default 16 frames.
    pub chrono_minimum_delay: i32,
    /// Distance (leptons) below which delay is forced to minimum
    /// (ChronoRangeMinimum= in [General]). Default 0.
    pub chrono_range_minimum: i32,

    /// Rules+F3C: PurifierBonus ReadDouble66FC70 -> FSTP float66FC75.
    /// Caller73E42B/522DCD multiplies count, these bits, then cargo amount.
    pub purifier_bonus: NativeF32Bits,
    /// AI virtual purifier counts indexed by difficulty
    /// (AIVirtualPurifiers= in [General]). Each entry is added to the AI
    /// player's real purifier count when computing the deposit bonus. INI
    /// convention is hardest-first, so the array is `[Brutal, Medium, Easy]`
    /// with the retail default `[4, 2, 0]`.
    pub ai_virtual_purifiers: [i32; 3],
    /// `[General] DisabledDisguiseDetectionPercent=`: an IntVector
    /// (`Rules+0xE10`, read at `0x0067020F` through `0x00475D70` with the
    /// field as its default; the constructor leaves it empty), indexed by the
    /// attacking House's difficulty (`+0x184`, hardest first). A computer
    /// attacker detects a blinking disguise when `RandomRanged(0, 99)` does
    /// not exceed its entry (Evaluate_Candidate `0x006F851C..0x006F854B`).
    pub disabled_disguise_detection_percent: Vec<i32>,

    // -- Survivor spawning on sell/destroy --
    /// Divisor to compute survivor count for Allied buildings (AlliedSurvivorDivisor= in [General]).
    /// RulesClass667371 default200; HowManySurvivors451330 clamps the
    /// signed refund/divisor quotient to1..5 (zero divisor suppresses crew).
    pub allied_survivor_divisor: i32,
    /// Divisor to compute survivor count for Soviet buildings (SovietSurvivorDivisor= in [General]).
    /// RulesClass66737B default100; stock RULESMD explicitly supplies250.
    pub soviet_survivor_divisor: i32,
    /// Divisor to compute survivor count for Third-side (Yuri) buildings (ThirdSurvivorDivisor= in [General]).
    /// RulesClass667385 default300; stock RULESMD explicitly supplies750.
    pub third_survivor_divisor: i32,
    /// `AlliedCrew=`/`SovietCrew=`/`ThirdCrew=` (Rules `+0xF78/+0xF7C/+0xF80`),
    /// `Technician=` (`+0xF6C`) and `Engineer=` (`+0xF70`): the InfantryTypes
    /// `TechnoClass::GetCrew @ 0x00707D20` and the building crew pick
    /// `0x0044EB10` return. Stock E1, E2, INIT, CTECH and ENGINEER.
    pub allied_crew: Option<String>,
    pub soviet_crew: Option<String>,
    pub third_crew: Option<String>,
    pub technician: Option<String>,
    pub engineer_infantry: Option<String>,
    /// Rules+17F8: constructor667793 initializes1.0f; General671DF7
    /// reads EngineerCaptureLevel with ReadDouble and stores an f32. The
    /// Infantry object-action threshold is independent of arrival repair.
    pub engineer_capture_level: NativeF32Bits,
    /// `CrewEscape=` (Rules `+0x5C0`, `ReadDouble`), the chance a crewed
    /// vehicle's crew escapes. Constructor default 0.5 (`0x00665E11..0x00665E17`).
    pub crew_escape: crate::util::native_x87::NativeF64Bits,
    /// `RefundPercent=` (Rules `+0x1738`, `ReadDouble`), the human-owner refund
    /// share `TechnoTypeClass::GetRefund @ 0x00711F60` applies. Constructor
    /// default 0.5 (`0x006675CE..0x006675D4`, ECX set at `0x00667190`).
    pub refund_percent: crate::util::native_x87::NativeF64Bits,
    /// `ShipSinkingWeight=` (Rules `+0x630`, `ReadDouble` at `0x0066F174`;
    /// constructor default 3.0 at `0x00665EE8`). A surface naval unit at
    /// least this `Weight=` sinks on water instead of exploding
    /// (`0x00737E00..0x00737E16`). Parsed like `Weight=` so the two compare
    /// as native doubles do on stock values (3.0 against 1..5).
    pub ship_sinking_weight: SimFixed,

    // -- Cliff/slope movement coefficients ([General]) --
    // Rules +0x768/+0x770/+0x778/+0x780, each `ReadDouble` over its current
    // value (0x66F213..0x66F2A9); the constructor stores 1.0 in all four
    // (0x6660BE..0x6660ED). Read by the Drive/Ship fresh speed publish
    // (0x4B3D4C..0x4B3DA6) and the per-cell speed chain.
    /// Tracked vehicle uphill coefficient (`TrackedUphill=`).
    pub tracked_uphill: NativeF64Bits,
    /// Tracked vehicle downhill coefficient (`TrackedDownhill=`).
    pub tracked_downhill: NativeF64Bits,
    /// Non-tracked (wheeled and other) vehicle uphill coefficient (`WheeledUphill=`).
    pub wheeled_uphill: NativeF64Bits,
    /// Non-tracked vehicle downhill coefficient (`WheeledDownhill=`).
    pub wheeled_downhill: NativeF64Bits,

    // -- Per-object draw-light offsets --
    /// Signed `[AudioVisual] ExtraUnitLight=` body-light offset (`1000 == 1.0`).
    pub extra_unit_light: i32,
    /// Signed `[AudioVisual] ExtraInfantryLight=` body-light offset (`1000 == 1.0`).
    pub extra_infantry_light: i32,
    /// Signed `[AudioVisual] ExtraAircraftLight=` draw offset (`1000 == 1.0`).
    pub extra_aircraft_light: i32,

    // -- Movement arrival --
    /// `Rules+0x1718`, `[General] CloseEnough=` in leptons: a blocked mover
    /// within this distance of its destination stops instead of repathing.
    /// Read by `RulesClass::ReadGeneral` at `0x00670EDD..0x00670EF7` through
    /// `CCINIClass::ReadRange 0x00474620` (cells x 256, chopped) over the
    /// current value; the constructor writes `0x280` (`0x00667588`). Retail
    /// `CloseEnough=2.25` is 576.
    pub close_enough: i32,
    /// `Rules+0x171C`, `[General] Stray=` in leptons: how far a team member
    /// may be from its team's centre or goal (`TeamClass::Regroup @
    /// 0x006EB870`, `Coordinate_Move @ 0x006EBAD0`). `ReadRange 0x00474620`
    /// at `0x00670E93` over the constructor's `0x200` (`0x00667592`). Retail
    /// `Stray=2.0` is 512.
    pub stray: i32,
    /// `Rules+0x1720`, `[General] RelaxedStray=` in leptons: `Stray=` for a
    /// team on script action 53 or 54. `ReadRange` at `0x00670EB2` over the
    /// constructor's `0x200` (`0x0066759C`). Retail `RelaxedStray=3.0` is 768.
    pub relaxed_stray: i32,
    /// `Rules+0x1724`, exact `[General] GuardModeStray=` in leptons.
    /// `ReadGeneral 0x00670EBD..0x00670EDD` uses `ReadRange 0x00474620`
    /// over the current value. Native construction does not write this field;
    /// Rust initializes the otherwise unspecified storage deterministically to0.
    /// Active retail's base General layer supplies2.0 (512 leptons).
    /// AreaGuard uses it only when its archived post is a Foot object
    /// (`0x004D6E74..0x004D6E89`), replacing the ordinary range-based leash.
    pub guard_mode_stray: i32,

    // -- Service depot / unit repair --
    /// `Rules+0x16E8`, `[General] URepairRate=` in minutes. ReadDouble
    /// at0x00670E4C over the current value, without a clamp. The constructor
    /// stores literal double .016 (0x3F90624DD2F1A9FC); an authored .016 is
    /// parsed as f32 and widened (0x3F90624DE0000000). Depot service compares
    /// progress against this retained value times900, without truncating it
    /// to a timer first (`0x0044BD32`, `0x0044BD44`).
    pub unit_repair_rate: f64,
    /// Rules+760; ctor6660AE stores3.0. ReadGeneral671858 uses the
    /// current double as ReadDouble5283D0 default, without scaling/clamping.
    pub charge_to_drain_ratio: NativeF64Bits,
    /// Hospital repair: Rules+16D8/16F0, ReadGeneral670DE9/670E57.
    pub infantry_repair_step: i32,
    pub infantry_repair_rate: f64,
    /// `[General] RepairStep=` — `RulesClass+0x16CC`, ReadInt over the
    /// constructor's 5 with no clamp (retail 8). TechnoTypeClass vt+0xB4
    /// (`0x00712120`) returns it: the health a repair tick adds, and the
    /// divisor of Strength in the repair step cost (`0x007120EC`).
    pub repair_step: i32,
    /// `[General] RepairPercent=` — `RulesClass+0x16D0`, ReadDouble
    /// (call `0x00670DBF`) over the constructor's .25 (retail `15%`, stored as
    /// 0x3FC3333333333333). The repair step cost multiplies the per-step
    /// share of the cost by it (`0x00712101`).
    pub repair_percent: f64,

    // -- Aircraft ammo reload --
    /// `[General] ReloadRate=` — `RulesClass+0x1508`, ReadDouble (call
    /// `0x00670C8E`) over the constructor's .05 (`0x0066738F`), in minutes.
    /// A dock's Mission_Repair returns `ftol(ReloadRate * 900)` after a visit
    /// that serviced a contact (`0x0044C92F`).
    pub reload_rate: f64,

    // -- Tiberium heal and transmogrify (read by gamemd, consumed by Ares) --
    /// `[General] TiberiumHeal=` (`RulesClass::TiberiumHeal`), minutes:
    /// constructor 1/60 (`0x3F91111111111111`), ReadDouble over the current
    /// value at `0x0066E6D2`. Ares' Foot heal (`10014BA0`) is its consumer.
    pub tiberium_heal_minutes: f64,
    /// `[General] TiberiumTransmogrify=` (`Rules+0x1688`), ReadInt over the
    /// current value at `0x00670F62`. gamemd's constructor leaves it unset;
    /// Ares 1002C990 (hook `0x66748A`) zeroes it. The visceroid chance of
    /// Ares' tiberium damage (`10049280`).
    pub tiberium_transmogrify: i32,
    /// `[General] SmallVisceroid=`, a UnitType pointer the RulesClass
    /// constructor nulls (`0x00665650`); ReadGeneral's ReadString (returning
    /// at `0x0066E6B0`) keeps it on an empty read, else UnitType
    /// FindOrAllocate. Published by `RulesTypeReferences`. Ares' tiberium
    /// death spawns it (`10049280` -> `TechnoExt::SpawnVisceroid @ 100468B0`).
    pub small_visceroid: Option<String>,
    /// `[General] LargeVisceroid=` (`Rules+0x10`), read the same way just
    /// before it (ReadString returning at `0x0066E677`). Ares' visceroid
    /// merge makes two small ones this type (`1004C530`).
    pub large_visceroid: Option<String>,

    // -- Movement delay timers --
    /// Retained [AI] PathDelay double, in minutes (Rules+1760).
    /// Convert at the timer producer with the native PC53 multiply/ftol.
    pub path_delay: f64,
    /// Ticks to wait when blocked by a friendly unit before aggressive repath
    /// ([AI] BlockagePathDelay). Native signed dword, in frames directly.
    /// When this timer expires, the unit re-pathfinds with urgency=2 (scatter).
    pub blockage_path_delay_ticks: i32,
    /// `Rules+0x1134`, `[AI] PowerSurplus=`: `RulesClass::ReadAI` reads it
    /// through `CCINIClass::ReadInteger 0x005276D0` (`0x00673A8B..0x00673AB0`)
    /// over the constructor's 50 (`0x00666DDC`). Stock code never uses it;
    /// Phobos' `EnablePowerSurplus` makes it the AI's spare power
    /// (`PowerState::power_surplus`).
    pub power_surplus: i32,
    /// Phobos `[AI] EnablePowerSurplus=` (`RulesExt`, default false): computer
    /// houses plan to keep `PowerSurplus` power spare.
    pub enable_power_surplus: bool,
    /// Phobos `[AI] PowerSurplus.ScaleToDrainAmount=` (`RulesExt`, default 0):
    /// above 0, the spare power is `PowerSurplus` times the house's drain over
    /// this amount.
    pub power_surplus_scale_to_drain_amount: i32,
    /// [General] AIAutoDeployFrameDelay, native signed DynamicVector at
    /// Rules+E2C (data+E30), indexed Hard/Normal/Easy by Infantry52155C.
    /// Constructor666932..666961 leaves it empty; stock15,25,100 is authored.
    pub ai_auto_deploy_frame_delay: Vec<i32>,
    /// Phobos `[General] InfantryAutoDeploy` (`RulesExt`, default false): the
    /// default for the TechnoType key that re-enables the undeployed
    /// `Approach_Target` arm at `0x00522434`.
    #[cfg(feature = "phobos")]
    pub infantry_auto_deploy: bool,
    /// Phobos `[General] AllowDamageOnSelf` (`RulesExt`, default false): the
    /// fallback for a warhead's `Nullable<bool>` when the warhead section
    /// leaves it unset.
    #[cfg(feature = "phobos")]
    pub allow_damage_on_self: bool,
    /// Phobos `[General] DecloakDamagedTargets` (`RulesExt`, default true):
    /// the fallback for a warhead's `Nullable<bool>` when the warhead section
    /// leaves it unset.
    #[cfg(feature = "phobos")]
    pub decloak_damaged_targets: bool,
    /// Phobos `[CombatDamage] ROF.RandomDelay` (`RulesExt`,
    /// `PartialVector2D<int>`, default `{0, 2}`): the full rearm's jitter
    /// range for a weapon that leaves its own key unset.
    #[cfg(feature = "phobos")]
    pub rof_random_delay: crate::rules::ini_value::PartialIntPair,
    /// `[AI] AIForcePredictionFudge=`, the signed DynamicVector at `Rules+0x9A8`
    /// (items `+0x9AC`) indexed Hard/Normal/Easy by the House difficulty:
    /// the percent by which the computer's estimate of its enemy's forces
    /// may err (`sim::ai_base_defense`). ReadAI `0x006732D9..0x00673314`
    /// copies the vector before the `0x00475D70` reader; the constructor
    /// (`0x00666373..0x00666388`) leaves it empty. Retail: 5,25,80.
    pub ai_force_prediction_fudge: Vec<i32>,
    /// `[General] AIPickWallDefensePercent=`, the signed DynamicVector at
    /// `Rules+0xDD4` (items `+0xDD8`) indexed Hard/Normal/Easy by the House
    /// difficulty: the chance that a defense node walls a building instead
    /// (`sim::ai_base_building`). ReadGeneral `0x006700C6..0x006700F7` copies
    /// the vector before the `0x00475D70` reader; the constructor
    /// (`0x0066689C..0x006668CA`) leaves it empty. Retail: 50,25,10.
    pub ai_pick_wall_defense_percent: Vec<i32>,

    // -- Cell scatter eligibility (CellClass::Scatter_Objects) --
    /// `PlayerScatter=` from `[CombatDamage]` — when set, an *unforced* cell
    /// scatter dispatches to every occupant regardless of who owns it. Stock
    /// `rulesmd.ini:900` says `no`, and the RulesClass constructor also clears
    /// the byte, so ordinarily only elite occupants and AI-owned occupants
    /// respond to an unforced scatter.
    pub player_scatter: bool,
    /// `PlayerReturnFire=` from `[CombatDamage]` (`Rules+0x17EC`, read at
    /// `0x0066CEBD`). When set, a human's objects retaliate on every mission;
    /// unset (the constructor's value and stock `rulesmd.ini:899`), they do so
    /// only on Guard, Area Guard or Patrol (`ShouldRetaliate 0x007089F7`),
    /// and a human's building hit by a non-Aircraft source turns its `+0x388`
    /// at random instead of taking the source (`0x00442A1B`).
    pub player_return_fire: bool,
    /// `Scatter=` from `[IQ]` — the house IQ level at or above which an
    /// occupant answers an *unforced* cell scatter. Stock `rulesmd.ini:3164`
    /// says `2`; the RulesClass constructor default is `3`.
    pub iq_scatter: i32,
    /// `[IQ] MaxIQLevels` stamped onto ordinary skirmish AI houses.
    pub max_iq_levels: i32,
    /// Signed `[IQ] Production` threshold for the House-update AI activation
    /// transaction. Native accepts the constructor default `5` or the parsed
    /// dword verbatim, without clamping it to `MaxIQLevels`.
    pub iq_production: i32,
    /// `[IQ] Harvester=` (`Rules+0x1458`, ReadInt at `0x00674394`, constructor
    /// 3 at `0x006671F8`; retail 2): the house IQ from which a computer house
    /// replaces its harvesters (`sim::ai_unit_choice`).
    pub iq_harvester: i32,
    /// `[IQ] SuperWeapons=` (`Rules+0x1438`, ReadInt at `0x00674282..
    /// 0x006742A2` with the field as its default; constructor 4 at
    /// `0x006671BB`): in game mode 0 the house IQ from which a computer house
    /// fires its superweapons (`AI_Building_Strategy 0x004FD77C..0x004FD797`).
    pub iq_super_weapons: i32,
    /// `[IQ] GuardArea=` (`Rules+0x1440`, ReadInt at `0x006742C7..0x006742DB`
    /// with the field as its default; constructor 4 at `0x006671C7`; retail
    /// 2): the house IQ from which a computer infantryman outside a team
    /// takes Area Guard when idle (`0x0051CCD2..0x0051CCFC`).
    pub iq_guard_area: i32,
    /// `[IQ] RepairSell` outer gate for BuildingClass repair/sell AI.
    pub iq_repair_sell: i32,
    /// `[IQ] AutoCrush=` (`Rules+0x1448`, ReadInt at `0x00674315`;
    /// constructor 2 at `0x006671DC`; retail 2): the house IQ from which a
    /// computer unit charges a crushable attacker
    /// (`UnitClass::Should_Crush_It @ 0x007438F0`).
    pub iq_auto_crush: i32,
    /// `[IQ] SellBack` gate for the red-health low-credit sell decision.
    pub iq_sell_back: i32,
    /// `[AI] CreditReserve` threshold. A latched AI building is considered
    /// for sale only while its owner's credits are strictly below this value.
    pub credit_reserve: i32,

    // -- Lightning Storm superweapon constants --
    // `RulesClass::ReadGeneral` reads each with ReadInteger over its current
    // value and no clamp (`0x00670F75..0x0067104D`); the constructor seeds
    // them at `0x0066767E..0x006676B2`.
    /// `[General] LightningStormDuration=` (`+0x179C`, constructor 900):
    /// frames a storm rages; `-1` never ends.
    pub lightning_storm_duration: i32,
    /// `[General] LightningDamage=` (`+0x1798`, constructor 200).
    pub lightning_damage: i32,
    /// `[General] LightningDeferment=` (`+0x1794`, constructor 250): frames
    /// between the launch and the storm.
    pub lightning_deferment: i32,
    /// `[General] LightningHitDelay=` (`+0x17A0`, constructor 90): a cloud
    /// over the storm's cell every frame this divides.
    pub lightning_hit_delay: i32,
    /// `[General] LightningScatterDelay=` (`+0x17A4`, constructor 10): a
    /// scattered cloud every frame this divides.
    pub lightning_scatter_delay: i32,
    /// `[General] LightningCellSpread=` (`+0x17A8`, constructor 10): a
    /// scattered cloud lands up to half this many cells off the storm's cell
    /// on each axis.
    pub lightning_cell_spread: i32,
    /// `[General] LightningSeparation=` (`+0x17AC`, constructor 3): the
    /// Manhattan distance in cells a scattered cloud keeps from every cloud
    /// present.
    pub lightning_separation: i32,
    /// [General] LightningWarhead (+17B4), constructor null.
    /// Retained factory binding from General671053; empty string means null.
    pub lightning_warhead: String,
    /// [General] WeatherConBoltExplosion (+2F4), constructor null;
    /// selected by SelectAnim48A59A for LightningWarhead.
    pub weather_con_bolt_explosion: String,
    /// [General] WeaponNullifyAnim (+350), constructor null; retained reader
    /// 66E2AF. Bullet46A2A1 uses it after AreaDamage returns IronCurtain (2).
    pub weapon_nullify_anim: String,
    /// Phobos `[AudioVisual] MissingCameo=` (default `XXICON.SHP`): the file
    /// a cameo with no art falls back to. The extension selects the loader —
    /// Phobos replaces the type's loaded `XXICON.SHP` handle when the name is
    /// a `.shp`, and blits a `.pcx` at draw time.
    #[cfg(feature = "phobos")]
    pub missing_cameo: String,
    /// Whether `[AudioVisual] AmbientChangeRate=` passes the native ordered,
    /// nonzero gate before conversion. A nonzero mod value can chop to a
    /// zero-frame interval; NaN fails the original FCOMP/C3 gate.
    pub ambient_change_rate_nonzero: bool,
    /// Lightning/global ambient transition interval in native frames:
    /// `ftol(AmbientChangeRate * 900)`.
    pub ambient_change_interval_frames: i32,
    /// Signed ambient scalar delta: `ftol(AmbientChangeStep * 100)`.
    pub ambient_change_step: i32,
    /// `[CombatDamage] IronCurtainDuration=` (`RulesClass+0xFE8`, ReadInt at
    /// `0x0066C646` defaulting to the field, which the constructor zeroes at
    /// `0x00666BC4`): the Iron Curtain's curtain in frames. Retail 750.
    pub iron_curtain_duration: i32,
    /// `[General] IronCurtainInvokeAnim=` (`RulesClass+0x348`, ReadString
    /// 0x80 at `0x0066E24C`; empty keeps the constructor's null type,
    /// `0x00665B1A`): the anim the Iron Curtain's launch builds over its cell.
    /// Retail `IRONBLST`.
    pub iron_curtain_invoke_anim: String,
    /// `[General] ChronoPlacement=` (`RulesClass+0x330`, ReadString 0x80 at
    /// `0x0066E095`, empty keeps the constructor's null type): the anim the
    /// Chronosphere's first click loops over its source cell
    /// (`SuperClass::CreateChronoAnim @ 0x006CB3A0`). Retail `CHRONOAR`.
    pub chrono_placement_anim: String,
    /// `[General] ChronoBlast=` (`+0x328`, `0x0066E112`): the Chrono Warp's
    /// anim over the source cell (`0x006CC674`). Retail `CHRONOFD`.
    pub chrono_blast_anim: String,
    /// `[General] ChronoBlastDest=` (`+0x32C`, `0x0066E151`): the Chrono
    /// Warp's anim over the destination cell (`0x006CC61A`). Retail
    /// `CHRONOTG`.
    pub chrono_blast_dest_anim: String,
    /// `[General] DominatorWarhead=` (`RulesClass+0x2F8`, ReadString 0x80
    /// then the warhead lookup at `0x0066DF72`; empty keeps the
    /// constructor's null warhead): the Psychic Dominator's area damage
    /// warhead (`PsyDom::MindControlArea @ 0x0053B080`). Retail `DominatorWH`.
    pub dominator_warhead: String,
    /// `[General] DominatorFirstAnim=` (`+0x2FC`, `0x0066DFB1`): the anim
    /// `PsyDom::Start @ 0x0053AE50` raises over the target. Start does
    /// nothing unless both Dominator anims are set. Retail `PDFXCLD`.
    pub dominator_first_anim: String,
    /// `[General] DominatorSecondAnim=` (`+0x300`, `0x0066DFEF`): the anim
    /// MindControlArea places on the target cell. Retail `PDFXLOC`.
    pub dominator_second_anim: String,
    /// `[General] DominatorFireAtPercentage=` (`+0x304`, ReadInt at
    /// `0x0066E020`, constructor 50): the share of the first anim's frames
    /// after which the Dominator strikes (`PsychicDominator::Process
    /// @ 0x0053AF40`).
    pub dominator_fire_at_percentage: i32,
    /// `[General] DominatorCaptureRange=` (`+0x308`, `0x0066E040`,
    /// constructor 2): MindControlArea's cell-spread radius, capped at 10.
    pub dominator_capture_range: i32,
    /// `[General] DominatorDamage=` (`+0x30C`, `0x0066E05F`, constructor
    /// 50): MindControlArea's area damage.
    pub dominator_damage: i32,
    /// `[General] IonBlast=` (`RulesClass+0x298`), the animation the Genetic
    /// Mutator launch constructs (`SuperClass::Launch 0x006CD8A5`). Retail
    /// `RING1`. The constructor default is a null type: no key, no animation.
    pub ion_blast_anim: String,
    /// General IonBeam, Rules+29C: ctor665A24=null, reader66DCDD..66DD0D.
    pub ion_beam_anim: String,
    /// CombatDamage IonCannonDamage, Rules+490: ctor665CD4=700;
    /// ReadInt5276D0/current default at66BBF8..66BC12, unclamped.
    pub ion_cannon_damage: i32,
    /// General spotlight parameters, projected from the retained Process owner.
    pub spotlight: SpotlightRules,
    // --- ForceShield ([General], RulesClass::ReadGeneral) ---
    /// `ForceShieldRadius=` (`Rules+0x17B8`, `0x0067109F`), in cells; the
    /// constructor's 10 (`0x006676C8`).
    pub force_shield_radius: i32,
    /// `ForceShieldDuration=` (`+0x17BC`, `0x006710BE`), the shield's frames;
    /// the constructor's 400 (`0x006676CE`).
    pub force_shield_duration: i32,
    /// `ForceShieldBlackoutDuration=` (`+0x17C0`, `0x006710DE`), the
    /// launcher's power outage; the constructor's 800 (`0x006676DD`).
    pub force_shield_blackout_duration: i32,
    /// `ForceShieldPlayFadeSoundTime=` (`+0x17C4`, `0x006710FE`): how long
    /// before the shield ends its type's `SpecialSound=` plays; the
    /// constructor's 50 (`0x006676E7`).
    pub force_shield_fade_sound_time: i32,
    /// `ForceShieldInvokeAnim=` (`+0x34C`, `0x0066E282`). The constructor's
    /// null type (`0x00665B20`): no key, no animation.
    pub force_shield_invoke_anim: String,
    // --- PsychicReveal ([CombatDamage]) ---
    /// `[CombatDamage] PsychicRevealRadius=` (`Rules+0xFEC`, ReadInteger at
    /// `0x0066C665` over the constructor's 3, `0x00666BCA`): the radius
    /// Launch case 11 hands `MapClass::RevealArea @ 0x005678E0`, which
    /// reveals nothing for 0 and clamps 11 and more to 10. Retail 15.
    pub psychic_reveal_radius: i32,
    // --- GeneticConverter ([SpecialWeapons] + [General]) ---
    /// `[SpecialWeapons] MutateWarhead=` (`+0xF98`, read at `0x006690BB`
    /// through WarheadTypeClass::FindOrAllocate `0x0075E3B0`): the warhead
    /// Launch case 9's walk hands each infantryman, as the ordered Rules
    /// processor resolved it. The constructor's null (`0x00666B3A`) and the
    /// factory's null for `none`: empty.
    pub mutate_warhead: String,
    /// `[SpecialWeapons] MutateExplosionWarhead=` (`+0xF9C`, `0x006690FA`):
    /// the warhead of case 9's area damage. The constructor's null
    /// (`0x00666B40`): empty.
    pub mutate_explosion_warhead: String,
    /// `[SpecialWeapons] EMPulseWarhead=` (`Rules+0xFA0`): the constructor
    /// stores null at `0x00666B46`. `RulesClass::ReadSpecialWeapons`
    /// (`0x00668FB0`) loads the prior pointer before `ReadString`
    /// (`0x00528A10`, call at `0x00669138`); an empty read keeps it, while a
    /// name goes through `WarheadTypeClass::FindOrAllocate`
    /// (`0x0075E3B0`, call at `0x00669145`). Retained for native state parity;
    /// no gameplay consumer is claimed.
    pub empulse_warhead: Option<String>,
    /// `[General] MutateExplosion=` (`+0x17C8`, `0x00671125`): case 9 deals
    /// area damage instead of walking the 3x3 block. The constructor's false
    /// (`0x006676ED`); retail sets yes.
    pub mutate_explosion: bool,
    /// Ordered `[General] MetallicDebris=` AnimType references from the native
    /// ReadGeneral128/factory pass. Constructor default is empty, not retail's
    /// authored list. Mirrors Rules+0x140 (data) / +0x14C (count).
    pub metallic_debris: Vec<String>,
    /// `[General] WeatherConClouds=` (`+0x2BC`, items `+0x2C0`, count
    /// `+0x2CC`), the same reader as `metallic_debris`: the Lightning Storm's
    /// clouds.
    pub weather_con_clouds: Vec<String>,
    /// `[General] WeatherConBolts=` (`+0x2D8`, items `+0x2DC`, count
    /// `+0x2E8`): the storm's bolts; the first one's image sets the clouds'
    /// height.
    pub weather_con_bolts: Vec<String>,
}

/// `[General] URepairRate=` default, in minutes.
const U_REPAIR_RATE_MINUTES: f64 = 0.016;

/// RulesClass665650, stores667371/66737B/667385 at+14F8/+14FC/+1500.
/// Original gamemd; ReadGeneral670C13..670C6C keeps prior values.
pub(crate) const DEFAULT_SURVIVOR_DIVISORS: [i32; 3] = [200, 100, 300];
/// RulesClass665D92 initializes retained General.MaximumCheerRate (+580).
pub(crate) const DEFAULT_MAXIMUM_CHEER_RATE: i32 = 1;
/// Phobos `RulesExt`'s `MissingCameo` constructor value: the stock
/// `XXICON.SHP` a type with no cameo art falls back to.
#[cfg(feature = "phobos")]
pub(crate) const DEFAULT_MISSING_CAMEO: &str = "XXICON.SHP";
/// RulesClass665650's literal AmbientChangeRate (0.2) and AmbientChangeStep
/// (0.1) doubles, before the AudioVisual authored reads.
pub(crate) const DEFAULT_AMBIENT_CHANGE_RATE: NativeF64Bits =
    NativeF64Bits::from_bits(0x3fc9_9999_9999_999a);
pub(crate) const DEFAULT_AMBIENT_CHANGE_STEP: NativeF64Bits =
    NativeF64Bits::from_bits(0x3fb9_9999_9999_999a);

pub(crate) const fn default_fog_rate() -> NativeF64Bits {
    // RulesClass constructor66740C/667416, before any authored f32 read.
    NativeF64Bits::from_bits(0x3fa9_9999_9999_999a)
}

pub(crate) const fn default_shroud_rate() -> NativeF64Bits {
    // RulesClass constructor6673FC/667402, independent of retail tuning.
    NativeF64Bits::from_bits(0x4010_0000_0000_0000)
}

/// Serde-only placeholder for the Phobos `MissingCameo` retained string: a
/// saved state written without the key reads back as the constructor value.
#[cfg(feature = "phobos")]
pub(crate) fn default_missing_cameo() -> String {
    DEFAULT_MISSING_CAMEO.to_owned()
}

impl Default for GeneralRules {
    fn default() -> Self {
        let (ambient_change_rate_nonzero, ambient_change_interval_frames, ambient_change_step) =
            ambient_change_terms(
                f64::from_bits(DEFAULT_AMBIENT_CHANGE_RATE.bits()),
                f64::from_bits(DEFAULT_AMBIENT_CHANGE_STEP.bits()),
            );
        Self {
            max_waypoint_path_length: 15,
            talk_bubble_time: 300,
            flash_frame_time: 7,
            weed_capacity: 0,
            radar_combat_flash_time: 21,
            detail: DetailRules::default(),
            pose_dir: 0,
            deploy_dir: 0,
            scroll_multiplier: 0.07,
            // RulesClass__Constructor @ 0x00665650 writes the double
            // 0x3F9EB851EB851EB8 to +0x14C8.
            savour_delay_minutes: 0.03,
            // RulesClass665650 initializes +16B8 before any AudioVisual read.
            gravity: 3,
            wind_direction: -1,
            veteran_sight: 0.0,
            veteran_combat: 1.0,
            veteran_speed: 1.0,
            veteran_rof: 1.0,
            difficulty_rows: [DifficultyRules::default(); 3],
            game_speed_bias: 1.0,
            veteran_armor: 1.0,
            curley_shuffle: false,
            enable_ai_build_limitation: false,
            disable_parallel_ai_queues: false,
            repair_rate_minutes: 0.016,
            // Rules665696..6656B0 initializes the current-default fields
            // before ReadGeneral66E6E2..66E74B. Retail overrides are separate.
            self_heal_infantry_frames: 1000,
            self_heal_infantry_amount: 1,
            self_heal_unit_frames: 1000,
            self_heal_unit_amount: 1,
            veteran_ratio: VETERAN_RATIO_DEFAULT,
            veteran_cap: VETERAN_CAP_DEFAULT,
            computer_base_defense_response: 3,
            // Native Rules+0xE48 constructor default; active retail overrides to 3.
            maximum_building_placement_failures: 5,
            placement_delay: 0.05,
            attack_delay: 5.0,
            campaign_money_delta_easy: 5000,
            campaign_money_delta_hard: -5000,
            ai_alternate_production_credit_cutoff: 1000,
            ai_restrict_replace_time: 500,
            team_delays: Vec::new(),
            total_ai_team_cap: Vec::new(),
            minimum_ai_defensive_teams: Vec::new(),
            maximum_ai_defensive_teams: Vec::new(),
            use_min_defense_rule: true,
            fill_earliest_team_probability: Vec::new(),
            dissolve_unfilled_team_delay: 5000,
            ai_safe_distance: 8,
            patrol_scan_minutes: NativeF64Bits::from_bits(0x3f90_624d_d2f1_a9fc),
            ai_trigger_success_weight_delta: NativeF64Bits::ONE,
            ai_trigger_failure_weight_delta: NativeF64Bits::from_bits((-1.0_f64).to_bits()),
            ai_trigger_track_record_coefficient: NativeF64Bits::ONE,
            harvesters_per_refinery: Vec::new(),
            ai_minor_super_ready_percent: NativeF32Bits::from_bits(0x3f4c_cccd),
            base_defense_delay_minutes: 0.25,
            suspend_priority: 20,
            suspend_delay_minutes: 2.0,
            leptons_per_sight_increase: 0,
            gap_radius: 10,
            reveal_by_height: true,
            ally_reveal: true,
            allies_allowed: false,
            ally_change_allowed: true,
            comp_easy_bonus: true,
            paranoid: true,
            tunnel_speed: SimFixed::ONE,
            missile_rot_var: 0.25,
            safety_altitude: 500,
            line_trail_color_override: [0; 3],
            chrono_beam_color: [0; 3],
            flight_level: 500,
            attacking_aircraft_sight_range: 5,
            display_cruise_height: 400, // Rules constructor665C3A
            jumpjet_smooth_facing: false,
            jumpjet_land_nearby: 0,
            ai_nearest_enemy: false,
            // Rules constructor 0x00665E2B..0x00665E81.
            hover_height: 120,
            hover_bob: NativeF64Bits::from_bits(0x403e_0000_0000_0000),
            hover_boost: NativeF64Bits::from_bits(0x3ff4_cccc_cccc_cccd),
            hover_acceleration: NativeF64Bits::from_bits(0x3f9e_b851_eb85_1eb8),
            hover_brake: NativeF64Bits::from_bits(0x3f9e_b851_eb85_1eb8),
            hover_dampen: NativeF64Bits::from_bits(0x3fe9_9999_9999_999a),
            // RulesClass ctor666163/66616D initializes +7B8/+7BC.
            parachute_max_fall_rate: -3,
            no_parachute_max_fall_rate: -100,
            paradrop_radius: 1024,
            parachute_shp: None,
            vehicle_parachute: None,
            amer_paradrop: ParadropList::default(),
            ally_paradrop: ParadropList::default(),
            sov_paradrop: ParadropList::default(),
            yuri_paradrop: ParadropList::default(),
            anim_to_infantry: Vec::new(),
            base_unit_types: Vec::new(),
            multiplayer_ai_cm: Vec::new(),
            ai_hate_delays: Vec::new(),
            pad_aircraft_types: Vec::new(),
            separate_aircraft: false,
            prism_type: None,
            drop_zone_anim: None,
            drop_zone_radius: 1024,
            prerequisite_proc_alternate: None,
            prism_support: PrismSupportRules::default(),
            building_types: GeneralBuildingTypes::default(),
            growth_rate_minutes: 2.0,
            // The RulesClass constructor's null pointers (+338..+344, +94 and
            // +BB4: `0x00665B02..0x00665B14`, `0x0066572F`, `0x006665CF`).
            warp_in: AnimRef {
                name: String::new(),
            },
            warp_out: AnimRef {
                name: String::new(),
            },
            warp_away: AnimRef {
                name: String::new(),
            },
            chrono_sparkle1: AnimRef {
                name: String::new(),
            },
            wake: AnimRef {
                name: String::new(),
            },
            move_flash: AnimRef {
                name: String::new(),
            },
            behind: None,
            // Slots 3/4/6..10 are the constructor's null pointers +9C..+B4
            // (`0x0066573B..0x0066575F`).
            infantry_death_anims: [
                None,
                None,
                None,
                None,
                None,
                Some("ELECTRO".to_string()),
                None,
                None,
                None,
                None,
                None,
            ],
            attack_cursor_on_disguise: false,
            allow_shrouded_subterannean_moves: false,
            default_mirage_disguises: Vec::new(),
            infantry_disguises: Default::default(),
            secret_lab_choices: Default::default(),
            infantry_blink_disguise_time: 0,
            tree_targeting: false,
            tree_strength: 25,
            condition_yellow: 0.5,
            condition_red: 0.5,
            cloaking_stages: 9,
            cloak_delay_frames: 0,
            cloak_sound: None,
            ares_decloak_sound: None,
            leave_bio_reactor_sound: None,
            enter_bio_reactor_sound: None,
            enter_grinder_sound: None,
            upgrade_veteran_sound: None,
            upgrade_elite_sound: None,
            slaves_free_sound: None,
            crate_money_sound: None,
            crate_reveal_sound: None,
            crate_fire_sound: None,
            crate_armour_sound: None,
            crate_speed_sound: None,
            crate_unit_sound: None,
            crate_promote_sound: None,
            elite_flash_timer: 0,
            idle_action_frequency: f64::from_bits(0x3fb5_3f7c_ed91_6873),
            force_shield_color: 0,
            laser_target_color: 0,
            #[cfg(feature = "phobos")]
            airstrike_line_color: [255, 0, 0],
            #[cfg(feature = "phobos")]
            airstrike_line_z_adjust: 0,
            iron_curtain_color: 0,
            berserk_color: 0,
            airstrike_abort_sound: None,
            airstrike_attack_voice: None,
            gate_sounds: [None, None],
            condition_red_sparking_probability: 0.02,
            condition_yellow_sparking_probability: 0.01,
            condition_red_spark_threshold: unit_draw_threshold(0.02),
            condition_yellow_spark_threshold: unit_draw_threshold(0.01),
            dumb_my_effectiveness_coefficient: 200.0,
            dumb_target_effectiveness_coefficient: 200.0,
            dumb_target_special_threat_coefficient: 200.0,
            dumb_target_strength_coefficient: 200.0,
            dumb_target_distance_coefficient: -1.0,
            threat_coefficient_defaults: Default::default(),
            enemy_house_threat_bonus: 0.0,
            threat_per_occupant: 5,
            normal_targeting_delay: 27,
            dead_bodies: Vec::new(),
            guard_area_targeting_delay: 36,
            building_garrisoned_sound: None,
            building_abandoned_sound: None,
            sell_sound: None,
            base_under_attack_sound: None,
            building_die_sound: None,
            building_damage_sound: None,
            create_unit_sound: None,
            create_infantry_sound: None,
            create_aircraft_sound: None,
            disk_laser_charge_up: None,
            lightning_sounds: Vec::new(),
            credit_ticks: Vec::new(),
            storm_sound: None,
            // `RulesClass::Constructor @ 0x006676BC` writes AL, and the last
            // definition of EAX before it is `0x00667202 MOV EAX,0x1`.
            lightning_print_text: true,
            dig_sound: None,
            dig_anim: None,
            nuke_take_off: String::new(),
            ai_super_defense_probability: Vec::new(),
            ai_super_defense_distance: 10,
            ai_super_defense_frames: 25,
            ai_ion_cannon_values: AiIonCannonValues::default(),
            psychic_dominator_activate_sound: None,
            genetic_mutator_activate_sound: None,
            psychic_reveal_activate_sound: None,
            spy_plane_camera: None,
            spy_plane_camera_frames: 16,
            chute_sound: None,
            gui_main_button_sound: None,
            gui_move_in_sound: None,
            gui_move_out_sound: None,
            shell_button_slide_sound: None,
            generic_click_sound: None,
            guard_sound: None,
            stop_sound: None,
            radar_on: None,
            radar_off: None,
            movie_on: None,
            movie_off: None,
            spy_sat_activation_sound: None,
            spy_sat_deactivation_sound: None,
            voice_ifv_repair: None,
            start_planning_mode_sound: None,
            end_planning_mode_sound: None,
            add_planning_mode_command_sound: None,
            place_beacon_sound: None,
            scatter_sound: None,
            scold_sound: None,
            generic_beep_sound: None,
            gui_checkbox_sound: None,
            ore_twinkle: None,
            ore_twinkle_chance: 50,
            gui_build_sound: None,
            building_slam: None,
            gui_tab_sound: None,
            incoming_message_sound: None,
            message_char_typed_sound: None,
            system_error_sound: None,
            options_changed_sound: None,
            player_left_sound: None,
            player_joined_sound: None,
            game_forming_sound: None,
            game_closed_sound: None,
            message_delay_minutes: crate::util::native_x87::NativeF64Bits::from_bits(
                0x3FE3_3333_3333_3333,
            ),
            shroud_grow: false,
            shroud_rate: default_shroud_rate(),
            fog_rate: default_fog_rate(),
            speak_delay_minutes: 0.0,
            gui_combo_open_sound: None,
            gui_combo_close_sound: None,
            gui_open_sound: None,
            gui_close_sound: None,
            bunker_walls_down_sound: None,
            bunker_walls_up_sound: None,
            direct_rocking_coefficient: SimFixed::lit("1.5"),
            fallback_coefficient: SimFixed::lit("0.1"),
            chrono_in_sound: Some("ChronoMinerTeleport".to_string()),
            chrono_out_sound: Some("ChronoMinerTeleport".to_string()),
            time_warp_out_sound: None,
            time_warp_in_sound: None,
            cheer_sound: None,
            maximum_cheer_rate: DEFAULT_MAXIMUM_CHEER_RATE,
            impact_water_sound: None,
            impact_land_sound: None,
            sinking_sound: None,
            score_anim_sound: None,
            construction_sound: None,
            building_repaired_sound: None,
            bomb_ticking_sound: None,
            bomb_attach_sound: None,
            damage_delay_minutes: 1.0,
            spy_power_blackout: 0,
            spy_money_steal_percent: NativeF32Bits::from_bits(0x3F00_0000),
            damage_fire_types: vec![],
            on_fire: Vec::new(),
            barrel_particle: None,
            barrel_explode: None,
            barrel_debris: Vec::new(),
            tiberium_short_scan: 0x600,
            tiberium_long_scan: 0x2000,
            buildup_time: f64::from_bits(0x3FA9_9999_9999_999A),
            slave_miner_short_scan: 0x500,
            slave_miner_slave_scan: 0x1000,
            drain_money_frame_delay: 100,
            drain_money_amount: 0,
            slave_miner_long_scan: 0x5000,
            slave_miner_scan_correction: 0x300,
            slave_miner_kick_frame_delay: 0x7FFF_FFFF,
            maximum_queued_objects: 5,
            building_production_queue: false,
            #[cfg(feature = "phobos")]
            building_type_selectable: false,
            ai_fire_sale: true,
            ai_fire_sale_delay: 0,
            ai_all_to_hunt: true,
            gather_when_mcv_deploy: true,
            #[cfg(feature = "phobos")]
            timer_blink_color_scheme: 5,
            harvester_too_far_distance: 5,
            approach_target_reset_multiplier: 1,
            chrono_harv_too_far_distance: 50,
            harvester_load_rate: 2,
            harvester_dump_frames: 15,
            chrono_delay: 60,
            chrono_reinf_delay: 180,
            chrono_distance_factor: 48,
            chrono_trigger: true,
            chrono_minimum_delay: 16,
            chrono_range_minimum: 0,
            purifier_bonus: NativeF32Bits::from_bits(0x3E80_0000),
            ai_virtual_purifiers: [4, 2, 0],
            disabled_disguise_detection_percent: Vec::new(),
            allied_survivor_divisor: DEFAULT_SURVIVOR_DIVISORS[0],
            soviet_survivor_divisor: DEFAULT_SURVIVOR_DIVISORS[1],
            third_survivor_divisor: DEFAULT_SURVIVOR_DIVISORS[2],
            allied_crew: None,
            soviet_crew: None,
            third_crew: None,
            technician: None,
            engineer_infantry: None,
            engineer_capture_level: NativeF32Bits::ONE,
            crew_escape: crate::util::native_x87::NativeF64Bits::from_bits(0x3fe0_0000_0000_0000),
            refund_percent: crate::util::native_x87::NativeF64Bits::from_bits(
                0x3fe0_0000_0000_0000,
            ),
            ship_sinking_weight: SimFixed::lit("3.0"),
            // RulesClass constructor 0x6660BE..0x6660ED.
            tracked_uphill: NativeF64Bits::ONE,
            tracked_downhill: NativeF64Bits::ONE,
            wheeled_uphill: NativeF64Bits::ONE,
            wheeled_downhill: NativeF64Bits::ONE,
            extra_unit_light: 0,
            extra_infantry_light: 0,
            extra_aircraft_light: 0,
            // RulesClass constructor 0x00667588.
            close_enough: 0x280,
            stray: 0x200,
            relaxed_stray: 0x200,
            guard_mode_stray: 0,
            // Rules constructor667530/53A retains the double, not frame ticks.
            unit_repair_rate: U_REPAIR_RATE_MINUTES,
            charge_to_drain_ratio: NativeF64Bits::from_bits(0x4008_0000_0000_0000),
            // Constructor66751A and667540: 1 and double0.016.
            infantry_repair_step: 1,
            infantry_repair_rate: 0.016,
            repair_step: 5,
            repair_percent: 0.25,
            #[cfg(feature = "phobos")]
            prone_speed_crawls: 0.67,
            #[cfg(feature = "phobos")]
            prone_speed_no_crawls: 1.5,
            // RulesClass constructor 0x0066738F.
            reload_rate: 0.05,
            tiberium_heal_minutes: f64::from_bits(0x3F91_1111_1111_1111),
            tiberium_transmogrify: 0,
            small_visceroid: None,
            large_visceroid: None,
            // PathDelay=.01 min = 0.6 sec = 9 ticks at 15 Hz.
            path_delay: 0.016,
            // BlockagePathDelay=60 frames (directly in frames, not minutes).
            blockage_path_delay_ticks: 60,
            power_surplus: 50,
            enable_power_surplus: false,
            power_surplus_scale_to_drain_amount: 0,
            ai_auto_deploy_frame_delay: Vec::new(),
            #[cfg(feature = "phobos")]
            infantry_auto_deploy: false,
            #[cfg(feature = "phobos")]
            allow_damage_on_self: false,
            #[cfg(feature = "phobos")]
            decloak_damaged_targets: true,
            #[cfg(feature = "phobos")]
            rof_random_delay: crate::rules::ini_value::PartialIntPair {
                low: 0,
                high: 2,
                values: 2,
            },
            ai_force_prediction_fudge: Vec::new(),
            ai_pick_wall_defense_percent: Vec::new(),
            // RulesClass constructor clears PlayerScatter and stores 3 into
            // [IQ] Scatter; stock rulesmd overrides the latter with 2.
            player_scatter: false,
            player_return_fire: false,
            iq_scatter: 3,
            max_iq_levels: 5,
            iq_production: 5,
            iq_harvester: 3,
            iq_super_weapons: 4,
            iq_guard_area: 4,
            iq_repair_sell: 3,
            iq_auto_crush: 2,
            iq_sell_back: 2,
            credit_reserve: 1000,
            cliff_back_impassability: read_cliff_back_impassability(None),
            lightning_storm_duration: 900,
            lightning_damage: 200,
            lightning_deferment: 250,
            lightning_hit_delay: 90,
            lightning_scatter_delay: 10,
            lightning_cell_spread: 10,
            lightning_separation: 3,
            lightning_warhead: String::new(),
            weather_con_bolt_explosion: String::new(),
            weapon_nullify_anim: String::new(),
            #[cfg(feature = "phobos")]
            missing_cameo: DEFAULT_MISSING_CAMEO.to_owned(),
            ambient_change_rate_nonzero,
            ambient_change_interval_frames,
            ambient_change_step,
            iron_curtain_duration: 0,
            iron_curtain_invoke_anim: String::new(),
            chrono_placement_anim: String::new(),
            chrono_blast_anim: String::new(),
            chrono_blast_dest_anim: String::new(),
            dominator_warhead: String::new(),
            dominator_first_anim: String::new(),
            dominator_second_anim: String::new(),
            // RulesClass constructor (`0x00665A91..0x00665AB8`).
            dominator_fire_at_percentage: 50,
            dominator_capture_range: 2,
            dominator_damage: 50,
            ion_blast_anim: String::new(),
            ion_beam_anim: String::new(),
            ion_cannon_damage: 700,
            spotlight: SpotlightRules::default(),
            force_shield_radius: 10,
            force_shield_duration: 400,
            force_shield_blackout_duration: 800,
            force_shield_fade_sound_time: 50,
            force_shield_invoke_anim: String::new(),
            psychic_reveal_radius: 3,
            mutate_warhead: String::new(),
            mutate_explosion_warhead: String::new(),
            empulse_warhead: None,
            mutate_explosion: false,
            metallic_debris: Vec::new(),
            weather_con_clouds: Vec::new(),
            weather_con_bolts: Vec::new(),
        }
    }
}

/// Fallback `VeteranRatio=` when `[General]` omits it.
///
/// UNCHECKED against `RulesClass`'s constructor initialiser for `+0x668`; stock
/// `rulesmd.ini` supplies `3`, so this only bites a mod that deletes the key.
const VETERAN_RATIO_DEFAULT: f64 = 3.0;
/// Fallback `VeteranCap=` when `[General]` omits it. Same UNCHECKED status as
/// [`VETERAN_RATIO_DEFAULT`]; stock supplies `2`.
const VETERAN_CAP_DEFAULT: f64 = 2.0;

/// `[AudioVisual] AmbientChangeRate=` and `AmbientChangeStep=` (Rules `+0x1668`,
/// `+0x1670`) as `LogicClass::PerTickUpdate`'s ambient fade reads them:
/// whether the rate is nonzero (`0x0055B351..0x0055B362`), the interval
/// `ftol(rate * 900)` (`0x0055B3D8..0x0055B3E4`) and the step
/// `ftol(step * 100)` (`0x0055B447..0x0055B453`).
pub(crate) fn ambient_change_terms(rate: f64, step: f64) -> (bool, i32, i32) {
    use crate::util::native_x87::{MaskedX87Chop53 as X, MaskedX87Ordering, NativeF64Bits};
    let rate = X::load_f64(NativeF64Bits::from_bits(rate.to_bits()));
    let step = X::load_f64(NativeF64Bits::from_bits(step.to_bits()));
    // FCOMP/test C3 at55B351..55B362 rejects zero and unordered. Math_ftol
    // returns the low dword of its signed64 result; a Rust saturating cast
    // differs for large/exceptional binary32 action inputs.
    let enabled = matches!(
        X::compare(rate, X::load_i32(0)),
        MaskedX87Ordering::Less | MaskedX87Ordering::Greater
    );
    (
        enabled,
        X::ftol_i32_low_masked(X::mul(rate, X::load_i32(900))),
        X::ftol_i32_low_masked(X::mul(step, X::load_i32(100))),
    )
}

/// `[General] CliffBackImpassability=` (`Rules+0x664`, a byte): ReadInt in
/// RulesClass::ReadGeneral (`0x0066D530`, `0x0066F1CB..0x0066F1E6`) over the
/// constructor's 0 (`0x00665F3B`), keeping the low byte. Retail RULESMD sets 2.
/// The early overlay branch of `CellClass::RecalcAttributes` (`0x0047D2B0`)
/// skips its neighbour test when the byte is 0 (`0x0047D386..0x0047D393`) and
/// writes Rock only when it is 2 (`0x0047D52F..0x0047D53E`).
///
/// Residual: gamemd constructs RulesClass once, in Init_Game (`0x0052BA60`,
/// call at `0x0052BAD1`), so rules that never set the key keep the byte of the
/// scenario played before. VERA starts every scenario at 0. No retail rules
/// omit the key.
pub(crate) fn read_cliff_back_impassability(general: Option<&IniSection>) -> u8 {
    general.map_or(0, |general| general.read_int("CliffBackImpassability", 0)) as u8
}

impl GeneralRules {
    pub fn allow_shrouded_subterannean_moves(&self) -> bool {
        self.allow_shrouded_subterannean_moves
    }

    pub(crate) fn threat_coefficient_defaults(
        &self,
    ) -> crate::rules::threat_coefficients::ThreatCoefficients {
        self.threat_coefficient_defaults
    }

    /// LogicClass::AI55B259..55B265 uses the shared ftol(minutes*900),
    /// without clamping negative, zero or exceptional conversion results.
    pub fn shroud_growth_interval_frames(&self) -> i32 {
        native_minutes_to_frames(f64::from_bits(self.shroud_rate.bits()))
    }

    /// LogicClass::AI55B304..55B310 converts the retained FogRate through
    /// the shared native minute-to-frame conversion before restarting its timer.
    pub fn fog_regrowth_interval_frames(&self) -> i32 {
        native_minutes_to_frames(f64::from_bits(self.fog_rate.bits()))
    }

    /// Original Rules+D00/D1C/D38 in Infantry, Unit, Building order.
    /// Retained canonical identities, including duplicate entries.
    pub fn secret_lab_choices(&self) -> &[Vec<String>; 3] {
        &self.secret_lab_choices
    }

    pub fn incoming_message_sound(&self) -> Option<&str> {
        self.incoming_message_sound.as_deref()
    }

    pub fn voice_ifv_repair(&self) -> Option<&str> {
        self.voice_ifv_repair.as_deref()
    }

    pub fn message_char_typed_sound(&self) -> Option<&str> {
        self.message_char_typed_sound.as_deref()
    }

    /// Centered UI rejection cue resolved by the fixed SOUNDMD owner.
    pub fn system_error_sound(&self) -> Option<&str> {
        self.system_error_sound.as_deref()
    }

    /// Centred main-menu typed-code cue resolved by the fixed SOUNDMD owner.
    pub fn options_changed_sound(&self) -> Option<&str> {
        self.options_changed_sound.as_deref()
    }

    pub fn player_left_sound(&self) -> Option<&str> {
        self.player_left_sound.as_deref()
    }

    pub fn player_joined_sound(&self) -> Option<&str> {
        self.player_joined_sound.as_deref()
    }

    pub fn game_forming_sound(&self) -> Option<&str> {
        self.game_forming_sound.as_deref()
    }

    pub fn game_closed_sound(&self) -> Option<&str> {
        self.game_closed_sound.as_deref()
    }

    pub fn wind_direction(&self) -> i32 {
        self.wind_direction
    }

    /// The Rules+714 reference resolved by the fixed SOUNDMD process binder.
    pub fn building_damage_sound(&self) -> Option<&str> {
        self.building_damage_sound.as_deref()
    }

    pub fn create_unit_sound(&self) -> Option<&str> {
        self.create_unit_sound.as_deref()
    }

    pub fn create_infantry_sound(&self) -> Option<&str> {
        self.create_infantry_sound.as_deref()
    }

    pub fn create_aircraft_sound(&self) -> Option<&str> {
        self.create_aircraft_sound.as_deref()
    }

    pub fn disk_laser_charge_up(&self) -> Option<&str> {
        self.disk_laser_charge_up.as_deref()
    }

    pub fn time_warp_out_sound(&self) -> Option<&str> {
        self.time_warp_out_sound.as_deref()
    }

    pub fn time_warp_in_sound(&self) -> Option<&str> {
        self.time_warp_in_sound.as_deref()
    }

    /// Signed retained input-rate value read by AllToCheer730F40.
    pub fn maximum_cheer_rate(&self) -> i32 {
        self.maximum_cheer_rate
    }

    pub(crate) fn ai_hate_delays(&self) -> &[i32] {
        &self.ai_hate_delays
    }

    pub(crate) fn cheer_sound(&self) -> Option<&str> {
        self.cheer_sound.as_deref()
    }

    /// Resolved command-bar opener cue. After SidebarInput6D08F5's defeat
    /// refusal, playback6D091A precedes the open-state test.
    pub fn gui_open_sound(&self) -> Option<&str> {
        self.gui_open_sound.as_deref()
    }

    /// Resolved command-bar close cue used by SidebarInput6D0860.
    pub fn gui_close_sound(&self) -> Option<&str> {
        self.gui_close_sound.as_deref()
    }

    /// Resolved campaign score emblem loop, `[AudioVisual] ScoreAnimSound=`.
    pub fn score_anim_sound(&self) -> Option<&str> {
        self.score_anim_sound.as_deref()
    }

    /// Drive4B3A65, Ship6A30B4 and Foot/Walk's timer producers retain the
    /// configured double until FLD/FMUL900/ftol7C5F00. In particular, authored
    /// decimal .01 is parsed through float by ReadDouble5283D0 and converts to
    /// 8, while 1% converts to9. No clamp, round-to-nearest or u16 narrowing.
    pub fn path_delay_ticks(&self) -> i32 {
        native_minutes_to_frames(self.path_delay)
    }

    /// Script16's FLD/FMUL900/ftol at6ECD46..6ECD57. Reuse the common,
    /// deterministic rules-minute converter rather than host float math.
    pub(crate) fn patrol_scan_frames(&self) -> i32 {
        native_minutes_to_frames(f64::from_bits(self.patrol_scan_minutes.bits()))
    }

    /// `ftol(PlacementDelay * 900.0)`, the computer Construction Yard's retry
    /// wait (`BuildingClass::Factory_AI @ 0x004501DB..0x004501F3`). Retail .05
    /// gives 45.
    pub fn placement_delay_frames(&self) -> i32 {
        native_minutes_to_frames(self.placement_delay)
    }

    /// `[AudioVisual] GateUp` (`up`) or `GateDown`.
    pub(crate) fn gate_sound(&self, up: bool) -> Option<&str> {
        self.gate_sounds[usize::from(up)].as_deref()
    }

    pub fn infantry_death_anim(&self, inf_death: i32) -> Option<&str> {
        usize::try_from(inf_death)
            .ok()
            .and_then(|slot| self.infantry_death_anims.get(slot))
            .and_then(Option::as_deref)
    }

    pub(super) fn from_ini(ini: &IniFile) -> Self {
        let defaults = Self::default();
        // Separate ReadJumpjetControls674467..67447E, no General gate.
        let display_cruise_height = ini
            .section("JumpjetControls")
            .map_or(defaults.display_cruise_height, |s| {
                s.read_int("CruiseHeight", defaults.display_cruise_height)
            });
        // VERA key, not a gamemd one; absent means the original's snaps.
        let jumpjet_smooth_facing = ini
            .section("JumpjetControls")
            .is_some_and(|s| s.read_bool("SmoothFacing", false));
        // VERA key; 0 keeps the original's hover over a taken cell.
        let jumpjet_land_nearby = ini
            .section("JumpjetControls")
            .map_or(0, |s| s.read_int("LandNearby", 0))
            .clamp(0, 8);
        // VERA key; absent keeps the original's first-house enemy search.
        let ai_nearest_enemy = ini
            .section("General")
            .is_some_and(|s| s.read_bool("AINearestEnemy", false));
        // AI IQ thresholds live in their own [IQ] read.
        let iq = ini.section_or_empty("IQ");
        // gamemd-derived: `RulesClass__ReadIQ @ 0x00674240` reads signed
        // `[IQ] Production` into `Rules+0x143C` at `0x006742C1`, independently
        // of the `[General]` pass and without clamping the parsed dword.
        let iq_production = iq.read_int("Production", defaults.iq_production);
        // `0x00674379..0x00674399`, the same section gate and reader.
        let iq_harvester = iq.read_int("Harvester", defaults.iq_harvester);
        // `0x00674282..0x006742A2`, the same section gate and reader.
        let iq_super_weapons = iq.read_int("SuperWeapons", defaults.iq_super_weapons);
        // RulesProcess668F56 reaches ReadAudioVisual6691E0 independently of
        // ReadGeneral.66B34B/66B372 pass AudioVisual to5283D0 and store raw
        // doubles in Rules+1708/+1700; a missing General section cannot skip them.
        let audio_visual = ini.section_or_empty("AudioVisual");
        let message_delay_minutes =
            audio_visual.read_double_bits("MessageDelay", defaults.message_delay_minutes);
        // Steam gamemd: AudioVisual66B478..66B498 reads SavourDelay
        // into Rules+14C8; Process668F56 reaches it independently of General.
        let savour_delay_minutes =
            audio_visual.read_double("SavourDelay", defaults.savour_delay_minutes);
        let pose_dir = audio_visual.read_int("PoseDir", defaults.pose_dir);
        let deploy_dir = audio_visual
            .read_int("DeployDir", defaults.deploy_dir >> 5)
            .wrapping_shl(5);
        let condition_yellow_native = audio_visual.read_double("ConditionYellow", 0.5);
        let condition_red_native = audio_visual.read_double("ConditionRed", defaults.condition_red);
        // Fixed-catalog IDs are read by the retained native Process owner;
        // this scalar projection alone has no catalog or sound-name reader.
        let bomb_ticking_sound = defaults.bomb_ticking_sound.clone();
        let bomb_attach_sound = defaults.bomb_attach_sound.clone();
        let ally_reveal = audio_visual.read_bool("AllyReveal", defaults.ally_reveal);
        let psychic_reveal_activate_sound = defaults.psychic_reveal_activate_sound.clone();
        // RulesProcess668F36 reaches ReadCombatDamage66BBB0 independently of
        // ReadGeneral too; it gates on its own section only.
        let psychic_reveal_radius = ini
            .section_or_empty("CombatDamage")
            .read_int("PsychicRevealRadius", defaults.psychic_reveal_radius);
        // Rules ReadAI6739E5..673A31 is independent of ReadGeneral.
        // Constructor66760E..66761E supplies PathDelay0.016 and blockage60.
        let ai = ini.section_or_empty("AI");
        let paranoid = ai.read_bool("Paranoid", defaults.paranoid);
        // RulesProcess668EC0 reaches 671EA0 independently of ReadGeneral too.
        let allies_allowed = ini
            .section_or_empty("MultiplayerDialogSettings")
            .read_bool("AlliesAllowed", defaults.allies_allowed);
        let ally_change_allowed = ini
            .section_or_empty("MultiplayerDialogSettings")
            .read_bool("AllyChangeAllowed", defaults.ally_change_allowed);
        let attack_delay = ai.read_double("AttackDelay", defaults.attack_delay);
        let path_delay = ai.read_double("PathDelay", defaults.path_delay);
        let blockage_path_delay_ticks =
            ai.read_int("BlockagePathDelay", defaults.blockage_path_delay_ticks);
        let power_surplus = ai.read_int("PowerSurplus", defaults.power_surplus);
        // Phobos `RulesExt::ExtData::LoadBeforeTypeData` (src/Ext/Rules/Body.cpp
        // at c783774a) reads these from hook 0x00679A15 in
        // `RulesClass::ReadTypeData`, which `Process` runs for every Rules pass.
        // Its `Read<bool, 1>` gives the stock reader's verdict, except that a
        // first comma piece of 128 bytes or more keeps the value.
        let enable_power_surplus =
            phobos_read(|| ai.read_bool("EnablePowerSurplus", defaults.enable_power_surplus));
        let power_surplus_scale_to_drain_amount = phobos_read(|| {
            ai.read_phobos_int(
                "PowerSurplus.ScaleToDrainAmount",
                defaults.power_surplus_scale_to_drain_amount,
            )
        });
        // ReadAudioVisual66B673..66B74E belongs to the retained Process
        // owner; a final merged INI cannot reproduce omitted-key defaults.
        let extra_unit_light = defaults.extra_unit_light;
        let extra_infantry_light = defaults.extra_infantry_light;
        let extra_aircraft_light = defaults.extra_aircraft_light;
        // Phobos `RulesExt` reads these `[AudioVisual]` keys independently of
        // RulesClass's `[General]` gate.
        #[cfg(feature = "phobos")]
        let airstrike_line_color =
            audio_visual.read_phobos_color("AirstrikeLineColor", defaults.airstrike_line_color);
        #[cfg(feature = "phobos")]
        let airstrike_line_z_adjust = crate::rules::ares_switch::not_ares_read(|| {
            audio_visual.read_ares_int("AirstrikeLineZAdjust", defaults.airstrike_line_z_adjust)
        });
        let Some(general) = ini.section("General") else {
            return Self {
                message_delay_minutes,
                savour_delay_minutes,
                pose_dir,
                deploy_dir,
                iq_production,
                iq_harvester,
                iq_super_weapons,
                display_cruise_height,
                jumpjet_smooth_facing,
                jumpjet_land_nearby,
                ai_nearest_enemy,
                condition_yellow: condition_yellow_native,
                condition_red: condition_red_native,
                path_delay,
                attack_delay,
                blockage_path_delay_ticks,
                power_surplus,
                enable_power_surplus,
                power_surplus_scale_to_drain_amount,
                bomb_ticking_sound,
                bomb_attach_sound,
                ally_reveal,
                allies_allowed,
                ally_change_allowed,
                paranoid,
                psychic_reveal_activate_sound,
                psychic_reveal_radius,
                extra_unit_light,
                extra_infantry_light,
                extra_aircraft_light,
                #[cfg(feature = "phobos")]
                airstrike_line_color,
                #[cfg(feature = "phobos")]
                airstrike_line_z_adjust,
                ..defaults
            };
        };
        // Combat-only globals are read in the late [CombatDamage] pass.
        let combat_damage = ini.section_or_empty("CombatDamage");
        // Slots 3/4/6..10 are RulesClass pointers the ordered Rules processor
        // publishes (`rules_type_references`).
        let mut infantry_death_anims = defaults.infantry_death_anims.clone();
        infantry_death_anims[5] = Some(
            ini.section("Animations")
                .and_then(|section| section.registry_ids().get(1).copied())
                .filter(|name| !name.is_empty())
                .unwrap_or("ELECTRO")
                .to_string(),
        );
        // [General] damage-Spark spawn probabilities (verified ctor defaults
        // 0.02/0.01; stock INI omits them). Raw doubles, not percentages. Bound
        // before `Self` so each band feeds both its stored value and its derived
        // integer roll threshold (a struct literal can't reference sibling fields).
        let condition_red_spark_prob: f64 =
            general.read_double("ConditionRedSparkingProbability", 0.02);
        let condition_yellow_spark_prob: f64 =
            general.read_double("ConditionYellowSparkingProbability", 0.01);
        Self {
            pose_dir,
            deploy_dir,
            max_waypoint_path_length: general
                .read_int("MaxWaypointPathLength", defaults.max_waypoint_path_length),
            flash_frame_time: general.read_int("FlashFrameTime", defaults.flash_frame_time),
            weed_capacity: general.read_int("WeedCapacity", defaults.weed_capacity),
            radar_combat_flash_time: general
                .read_int("RadarCombatFlashTime", defaults.radar_combat_flash_time),
            scroll_multiplier: audio_visual
                .read_double("ScrollMultiplier", defaults.scroll_multiplier),
            savour_delay_minutes,
            condition_red_sparking_probability: condition_red_spark_prob,
            condition_yellow_sparking_probability: condition_yellow_spark_prob,
            condition_red_spark_threshold: unit_draw_threshold(condition_red_spark_prob),
            condition_yellow_spark_threshold: unit_draw_threshold(condition_yellow_spark_prob),
            dumb_my_effectiveness_coefficient: general.read_double(
                "DumbMyEffectivenessCoefficient",
                defaults.dumb_my_effectiveness_coefficient,
            ),
            dumb_target_effectiveness_coefficient: general.read_double(
                "DumbTargetEffectivenessCoefficient",
                defaults.dumb_target_effectiveness_coefficient,
            ),
            dumb_target_special_threat_coefficient: general.read_double(
                "DumbTargetSpecialThreatCoefficient",
                defaults.dumb_target_special_threat_coefficient,
            ),
            dumb_target_strength_coefficient: general.read_double(
                "DumbTargetStrengthCoefficient",
                defaults.dumb_target_strength_coefficient,
            ),
            dumb_target_distance_coefficient: general.read_double(
                "DumbTargetDistanceCoefficient",
                defaults.dumb_target_distance_coefficient,
            ),
            // The ordered processor owns these current-default reads.
            threat_coefficient_defaults: defaults.threat_coefficient_defaults,
            enemy_house_threat_bonus: general
                .read_double("EnemyHouseThreatBonus", defaults.enemy_house_threat_bonus),
            threat_per_occupant: general
                .read_int("ThreatPerOccupant", defaults.threat_per_occupant),
            // The ordered Rules processor publishes this and the other
            // RulesClass Type pointers and lists (`rules_type_references`).
            dead_bodies: defaults.dead_bodies.clone(),
            // Passive-scan cadence, in frames. Both keys are present in stock
            // rulesmd.ini with exactly the constructor defaults (27 / 36); read
            // them rather than hardcoding so a mod's values take effect.
            normal_targeting_delay: general
                .read_int("NormalTargetingDelay", defaults.normal_targeting_delay),
            guard_area_targeting_delay: general.read_int(
                "GuardAreaTargetingDelay",
                defaults.guard_area_targeting_delay,
            ),
            veteran_sight: general.read_double("VeteranSight", defaults.veteran_sight),
            veteran_combat: general.read_double("VeteranCombat", defaults.veteran_combat),
            veteran_speed: general.read_double("VeteranSpeed", defaults.veteran_speed),
            veteran_rof: general.read_double("VeteranROF", defaults.veteran_rof),
            game_speed_bias: general.read_double("GameSpeedBias", defaults.game_speed_bias),
            veteran_armor: general.read_double("VeteranArmor", 1.0),
            curley_shuffle: general.read_bool("CurleyShuffle", defaults.curley_shuffle),
            // ext173 reads it with the current byte as default into
            // `Rules+0x641` (`0xC2D132..0xC2D151`, `ReadBooleanByte 0x5295F0`).
            enable_ai_build_limitation: crate::rules::NPATCH_ENABLED
                && general.read_bool(
                    "EnableAIBuildLimitation",
                    defaults.enable_ai_build_limitation,
                ),
            // NPSE reads it with the current field as default into Rules+0x2D
            // (C30303..C3031A); the constructor's 66569B store leaves 0. NPExt
            // ext173 reads its name of the same global switch into Rules+0x640
            // (reader C2BDDA..C2BDE7, gate C2D482), so an absent or unparsable
            // NPSE value falls through to that name.
            disable_parallel_ai_queues: crate::rules::NPATCH_ENABLED
                && general.read_bool(
                    "DisableParallelAIQueues",
                    general
                        .read_bool_value("DisableAIParallelProduction")
                        .unwrap_or(defaults.disable_parallel_ai_queues),
                ),
            repair_rate_minutes: general.read_double("RepairRate", defaults.repair_rate_minutes),
            // `RulesClass+0x30/+0x34/+0x38/+0x3C`, read by the General reader at
            // `0x0066D530`. ReadInt66E6F3/70D/727/740 retains each current
            // field, initially constructor1000/1/1000/1, without a clamp.
            self_heal_infantry_frames: general
                .read_int("SelfHealInfantryFrames", defaults.self_heal_infantry_frames),
            self_heal_infantry_amount: general
                .read_int("SelfHealInfantryAmount", defaults.self_heal_infantry_amount),
            self_heal_unit_frames: general
                .read_int("SelfHealUnitFrames", defaults.self_heal_unit_frames),
            self_heal_unit_amount: general
                .read_int("SelfHealUnitAmount", defaults.self_heal_unit_amount),
            veteran_ratio: general.read_double("VeteranRatio", VETERAN_RATIO_DEFAULT),
            veteran_cap: general.read_double("VeteranCap", VETERAN_CAP_DEFAULT),
            computer_base_defense_response: ai.read_int(
                "ComputerBaseDefenseResponse",
                defaults.computer_base_defense_response,
            ),
            // Signed [General] binding for native Rules+0xE48. The verified
            // report establishes default 5 and active-retail override 3.
            maximum_building_placement_failures: general.read_int(
                "MaximumBuildingPlacementFailures",
                defaults.maximum_building_placement_failures,
            ),
            placement_delay: general.read_double("PlacementDelay", defaults.placement_delay),
            attack_delay,
            campaign_money_delta_easy: general
                .read_int("CampaignMoneyDeltaEasy", defaults.campaign_money_delta_easy),
            campaign_money_delta_hard: general
                .read_int("CampaignMoneyDeltaHard", defaults.campaign_money_delta_hard),
            ai_alternate_production_credit_cutoff: general.read_int(
                "AIAlternateProductionCreditCutoff",
                defaults.ai_alternate_production_credit_cutoff,
            ),
            ai_restrict_replace_time: general
                .read_int("AIRestrictReplaceTime", defaults.ai_restrict_replace_time),
            team_delays: general.read_int_list("TeamDelays").unwrap_or_default(),
            total_ai_team_cap: general.read_int_list("TotalAITeamCap").unwrap_or_default(),
            minimum_ai_defensive_teams: general
                .read_int_list("MinimumAIDefensiveTeams")
                .unwrap_or_default(),
            maximum_ai_defensive_teams: general
                .read_int_list("MaximumAIDefensiveTeams")
                .unwrap_or_default(),
            use_min_defense_rule: general
                .read_bool("UseMinDefenseRule", defaults.use_min_defense_rule),
            fill_earliest_team_probability: general
                .read_int_list("FillEarliestTeamProbability")
                .unwrap_or_default(),
            dissolve_unfilled_team_delay: general.read_int(
                "DissolveUnfilledTeamDelay",
                defaults.dissolve_unfilled_team_delay,
            ),
            ai_safe_distance: general.read_int("AISafeDistance", defaults.ai_safe_distance),
            ai_trigger_success_weight_delta: general.read_double_bits(
                "AITriggerSuccessWeightDelta",
                defaults.ai_trigger_success_weight_delta,
            ),
            ai_trigger_failure_weight_delta: general.read_double_bits(
                "AITriggerFailureWeightDelta",
                defaults.ai_trigger_failure_weight_delta,
            ),
            ai_trigger_track_record_coefficient: general.read_double_bits(
                "AITriggerTrackRecordCoefficient",
                defaults.ai_trigger_track_record_coefficient,
            ),
            harvesters_per_refinery: general
                .read_int_list("HarvestersPerRefinery")
                .unwrap_or_default(),
            ai_minor_super_ready_percent: general.read_double_to_float(
                "AIMinorSuperReadyPercent",
                defaults.ai_minor_super_ready_percent,
            ),
            base_defense_delay_minutes: general
                .read_double("BaseDefenseDelay", defaults.base_defense_delay_minutes),
            suspend_priority: general.read_int("SuspendPriority", defaults.suspend_priority),
            suspend_delay_minutes: general
                .read_double("SuspendDelay", defaults.suspend_delay_minutes),
            leptons_per_sight_increase: general.read_int("LeptonsPerSightIncrease", 0),
            gap_radius: general.read_int("GapRadius", 10),
            reveal_by_height: general.read_bool("RevealByHeight", true),
            ally_reveal,
            allies_allowed,
            ally_change_allowed,
            comp_easy_bonus: defaults.comp_easy_bonus,
            paranoid,
            // Saturating: an out-of-range literal scans as infinity.
            tunnel_speed: SimFixed::saturating_from_num(
                general.read_double("TunnelSpeed", 1.0) as f32
            ),
            flight_level: general.read_int("FlightLevel", 500),
            attacking_aircraft_sight_range: general.read_int(
                "AttackingAircraftSightRange",
                defaults.attacking_aircraft_sight_range,
            ),
            display_cruise_height,
            jumpjet_smooth_facing,
            jumpjet_land_nearby,
            ai_nearest_enemy,
            // ReadGeneral 0x0066EDC5..0x0066EE83 reads these into their
            // fields with the current value as default (%-aware ReadDouble).
            hover_height: general.read_int("HoverHeight", defaults.hover_height),
            hover_bob: general.read_double_bits("HoverBob", defaults.hover_bob),
            hover_boost: general.read_double_bits("HoverBoost", defaults.hover_boost),
            hover_acceleration: general
                .read_double_bits("HoverAcceleration", defaults.hover_acceleration),
            hover_brake: general.read_double_bits("HoverBrake", defaults.hover_brake),
            hover_dampen: general.read_double_bits("HoverDampen", defaults.hover_dampen),
            paradrop_radius: general.read_int("ParadropRadius", 1024),
            parachute_shp: defaults.parachute_shp.clone(),
            // The ordered RulesTypeReferences owner publishes the retained
            // NPSE ReadGeneral pointer (C31005..C31017), like Parachute.
            vehicle_parachute: defaults.vehicle_parachute.clone(),
            amer_paradrop: ParadropList::read_counts(general, "AmerParaDropNum"),
            ally_paradrop: ParadropList::read_counts(general, "AllyParaDropNum"),
            sov_paradrop: ParadropList::read_counts(general, "SovParaDropNum"),
            yuri_paradrop: ParadropList::read_counts(general, "YuriParaDropNum"),
            // The processed RulesClass vector is authoritative.
            anim_to_infantry: Vec::new(),
            base_unit_types: defaults.base_unit_types.clone(),
            multiplayer_ai_cm: general
                .read_int_list("MultiplayerAICM")
                .unwrap_or(defaults.multiplayer_ai_cm),
            // The retained RulesClass vector is projected after all passes.
            ai_hate_delays: defaults.ai_hate_delays,
            pad_aircraft_types: defaults.pad_aircraft_types.clone(),
            separate_aircraft: general.read_bool("SeparateAircraft", defaults.separate_aircraft),
            // Only the ordered Process projection binds these references/reads.
            drop_zone_anim: defaults.drop_zone_anim,
            drop_zone_radius: defaults.drop_zone_radius,
            growth_rate_minutes: general.read_double("GrowthRate", 2.0) as f32,
            // Published from the retained General pass owner.
            attack_cursor_on_disguise: defaults.attack_cursor_on_disguise,
            allow_shrouded_subterannean_moves: defaults.allow_shrouded_subterannean_moves,
            // Filled from the native ordered TerrainType list owner after
            // processing, including allocation/none-token/retention semantics.
            default_mirage_disguises: Vec::new(),
            infantry_disguises: Default::default(),
            secret_lab_choices: Default::default(),
            // Rules671D80: signed ReadInt, constructor665650 defaults0.
            infantry_blink_disguise_time: general.read_int("InfantryBlinkDisguiseTime", 0),
            tree_targeting: combat_damage.read_bool("TreeTargeting", false),
            // The ordered General owner retains this across Type resets and
            // projects it before Terrain parsing; this is only the ctor value.
            tree_strength: defaults.tree_strength,
            condition_yellow: condition_yellow_native,
            condition_red: condition_red_native,
            cloaking_stages: general.read_int("CloakingStages", 9),
            cloak_delay_frames: (general.read_double("CloakDelay", 0.0) * 900.0)
                .trunc()
                .clamp(i32::MIN as f64, i32::MAX as f64) as i32,
            // RulesClass::ReadAudioVisual @ 0x006691E0 resolves CloakSound into
            // RulesClass+0x6A0. Retain the name at the data boundary; an absent
            // or empty key leaves the native invalid-index/no-play behavior.
            cloak_sound: audio_visual
                .read_name("CloakSound", 0x80)
                .map(str::to_owned),
            ares_decloak_sound: None,
            // The fixed SOUNDMD binder resolves Rules+0x274 as a Voc index.
            // Keep the constructor's invalid index until that owner binds it.
            leave_bio_reactor_sound: None,
            enter_bio_reactor_sound: None,
            enter_grinder_sound: None,
            // The fixed SOUNDMD binder resolves Rules+0x228/+0x22C as Voc
            // indexes; keep the constructor's invalid index until it binds them.
            upgrade_veteran_sound: None,
            upgrade_elite_sound: None,
            slaves_free_sound: audio_visual
                .read_name("SlavesFreeSound", 0x80)
                .map(str::to_owned),
            crate_money_sound: audio_visual
                .read_name("CrateMoneySound", 0x80)
                .map(str::to_owned),
            crate_reveal_sound: audio_visual
                .read_name("CrateRevealSound", 0x80)
                .map(str::to_owned),
            crate_fire_sound: audio_visual
                .read_name("CrateFireSound", 0x80)
                .map(str::to_owned),
            crate_armour_sound: audio_visual
                .read_name("CrateArmourSound", 0x80)
                .map(str::to_owned),
            crate_speed_sound: audio_visual
                .read_name("CrateSpeedSound", 0x80)
                .map(str::to_owned),
            crate_unit_sound: audio_visual
                .read_name("CrateUnitSound", 0x80)
                .map(str::to_owned),
            crate_promote_sound: audio_visual
                .read_name("CratePromoteSound", 0x80)
                .map(str::to_owned),
            talk_bubble_time: defaults.talk_bubble_time,
            elite_flash_timer: audio_visual.read_int("EliteFlashTimer", defaults.elite_flash_timer),
            idle_action_frequency: audio_visual
                .read_double("IdleActionFrequency", defaults.idle_action_frequency),
            airstrike_abort_sound: None,
            airstrike_attack_voice: None,
            gate_sounds: [None, None],
            building_garrisoned_sound: audio_visual
                .read_name("BuildingGarrisonedSound", 0x80)
                .map(str::to_owned),
            building_abandoned_sound: None,
            sell_sound: audio_visual.read_name("SellSound", 0x80).map(str::to_owned),
            base_under_attack_sound: audio_visual
                .read_name("BaseUnderAttackSound", 0x80)
                .map(str::to_owned),
            building_die_sound: audio_visual
                .read_name("BuildingDieSound", 0x80)
                .map(str::to_owned),
            building_damage_sound: None,
            create_unit_sound: None,
            create_infantry_sound: None,
            create_aircraft_sound: None,
            disk_laser_charge_up: None,
            // `CCINIClass::ReadSoundList @ 0x00525430`: `ReadString(..., "",
            // buf, 0x80)` first (so the whole value is byte-cut and trimmed
            // once), then `strtok` on `","` (`0x00817F70`) with the tokens
            // left untrimmed. Resolution against the Voc registry
            // (`VocClass::FindPtrByName`, which drops unresolvable names) is
            // deferred to the app layer, where the registry lives.
            lightning_sounds: audio_visual
                .read_list("LightningSounds", 0x80)
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
            credit_ticks: audio_visual
                .read_list("CreditTicks", 0x80)
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
            storm_sound: audio_visual
                .read_name("StormSound", 0x80)
                .map(str::to_owned),
            // `RulesClass::ReadGeneral @ 0x0067107F` passes the field's own
            // current value as the default, so an absent key keeps the
            // constructor's `true` (see the field doc).
            lightning_print_text: general.read_bool("LightningPrintText", true),
            dig_sound: audio_visual.read_name("DigSound", 0x80).map(str::to_owned),
            // Published by the Rules processor's `Dig` pointer
            // (`rules_type_references`).
            dig_anim: None,
            nuke_take_off: defaults.nuke_take_off.clone(),
            ai_super_defense_probability: general
                .read_int_list("AISuperDefenseProbability")
                .unwrap_or_default(),
            ai_super_defense_distance: general.read_range("AISuperDefenseDistance", 10),
            ai_super_defense_frames: general
                .read_int("AISuperDefenseFrames", defaults.ai_super_defense_frames),
            ai_ion_cannon_values: AiIonCannonValues::read(general),
            psychic_dominator_activate_sound: audio_visual
                .read_name("PsychicDominatorActivateSound", 0x80)
                .map(str::to_owned),
            genetic_mutator_activate_sound: audio_visual
                .read_name("GeneticMutatorActivateSound", 0x80)
                .map(str::to_owned),
            psychic_reveal_activate_sound,
            // Constructor -1 until the fixed SOUNDMD catalog resolves it.
            spy_plane_camera: None,
            spy_plane_camera_frames: audio_visual
                .read_int("SpyPlaneCameraFrames", defaults.spy_plane_camera_frames),
            chute_sound: audio_visual
                .read_name("ChuteSound", 0x80)
                .map(str::to_owned),
            bunker_walls_down_sound: audio_visual
                .read_name("BunkerWallsDownSound", 0x80)
                .map(str::to_owned),
            bunker_walls_up_sound: audio_visual
                .read_name("BunkerWallsUpSound", 0x80)
                .map(str::to_owned),
            gui_main_button_sound: audio_visual
                .read_name("GUIMainButtonSound", 0x80)
                .map(str::to_owned),
            gui_move_in_sound: audio_visual
                .read_name("GUIMoveInSound", 0x80)
                .map(str::to_owned),
            gui_move_out_sound: audio_visual
                .read_name("GUIMoveOutSound", 0x80)
                .map(str::to_owned),
            shell_button_slide_sound: audio_visual
                .read_name("ShellButtonSlideSound", 0x80)
                .map(str::to_owned),
            // Native constructor -1; resolved by the fixed SOUNDMD binder.
            generic_click_sound: None,
            // Native constructor -1; resolved by the fixed SOUNDMD binder.
            guard_sound: None,
            stop_sound: None,
            radar_on: None,
            radar_off: None,
            movie_on: None,
            movie_off: None,
            spy_sat_activation_sound: None,
            spy_sat_deactivation_sound: None,
            start_planning_mode_sound: None,
            end_planning_mode_sound: None,
            add_planning_mode_command_sound: None,
            place_beacon_sound: None,
            scatter_sound: None,
            scold_sound: None,
            generic_beep_sound: audio_visual
                .read_name("GenericBeep", 0x80)
                .map(str::to_owned),
            gui_checkbox_sound: audio_visual
                .read_name("GUICheckboxSound", 0x80)
                .map(str::to_owned),
            ore_twinkle: defaults.ore_twinkle.clone(),
            ore_twinkle_chance: audio_visual
                .read_int("OreTwinkleChance", defaults.ore_twinkle_chance),
            // ReadAudioVisual owns these independently of ReadGeneral.
            // The process projection supplies retained fixed Voc bindings.
            gui_build_sound: None,
            building_slam: None,
            gui_tab_sound: None,
            // The fixed sound registry binds these readers below, including
            // current-ID retention through the actual Rules pass history.
            voice_ifv_repair: None,
            incoming_message_sound: None,
            message_char_typed_sound: None,
            system_error_sound: None,
            options_changed_sound: None,
            player_left_sound: None,
            player_joined_sound: None,
            game_forming_sound: None,
            game_closed_sound: None,
            message_delay_minutes,
            // Projected from the retained AudioVisual pass after processing.
            shroud_grow: defaults.shroud_grow,
            shroud_rate: defaults.shroud_rate,
            fog_rate: defaults.fog_rate,
            speak_delay_minutes: audio_visual.read_double("SpeakDelay", 0.0),
            gui_combo_open_sound: audio_visual
                .read_name("GUIComboOpenSound", 0x80)
                .map(str::to_owned),
            gui_combo_close_sound: audio_visual
                .read_name("GUIComboCloseSound", 0x80)
                .map(str::to_owned),
            // Constructor -1 until the fixed SOUNDMD binder resolves each
            // reached ReadString128 value, including failed-read retention.
            gui_open_sound: None,
            gui_close_sound: None,
            // Float fields (`FSTP dword` at `0x0066B8B1`/`0x0066B8DB`),
            // saturating: an out-of-range literal scans as infinity.
            direct_rocking_coefficient: SimFixed::saturating_from_num(
                audio_visual.read_float("DirectRockingCoefficient", 1.5),
            ),
            fallback_coefficient: SimFixed::saturating_from_num(
                audio_visual.read_float("FallBackCoefficient", 0.1),
            ),
            // ChronoInSound/ChronoOutSound live in [AudioVisual], not [General].
            // No hardcoded fallback: a genuinely-absent key means no fallback
            // sound (silence), matching gamemd. Stock ships these keys present
            // (= ChronoMinerTeleport), so stock audio is unchanged.
            chrono_in_sound: audio_visual
                .read_name("ChronoInSound", 0x80)
                .map(str::to_owned),
            chrono_out_sound: audio_visual
                .read_name("ChronoOutSound", 0x80)
                .map(str::to_owned),
            time_warp_out_sound: None,
            time_warp_in_sound: None,
            cheer_sound: None,
            impact_water_sound: audio_visual
                .read_name("ImpactWaterSound", 0x80)
                .map(str::to_owned),
            impact_land_sound: audio_visual
                .read_name("ImpactLandSound", 0x80)
                .map(str::to_owned),
            // Constructor -1 until the fixed SOUNDMD catalog resolves it.
            sinking_sound: None,
            score_anim_sound: None,
            construction_sound: None,
            building_repaired_sound: None,
            bomb_ticking_sound,
            bomb_attach_sound,
            warp_in: defaults.warp_in.clone(),
            warp_out: defaults.warp_out.clone(),
            warp_away: defaults.warp_away.clone(),
            chrono_sparkle1: defaults.chrono_sparkle1.clone(),
            wake: defaults.wake.clone(),
            move_flash: defaults.move_flash.clone(),
            behind: defaults.behind,
            infantry_death_anims,
            damage_delay_minutes: general.read_double("DamageDelay", 1.0) as f32,
            spy_power_blackout: general.read_int("SpyPowerBlackout", defaults.spy_power_blackout),
            spy_money_steal_percent: general
                .read_double_to_float("SpyMoneyStealPercent", defaults.spy_money_steal_percent),
            damage_fire_types: defaults.damage_fire_types.clone(),
            on_fire: defaults.on_fire.clone(),
            barrel_particle: defaults.barrel_particle.clone(),
            barrel_explode: defaults.barrel_explode.clone(),
            barrel_debris: defaults.barrel_debris.clone(),
            tiberium_short_scan: general.read_range("TiberiumShortScan", 0x600),
            tiberium_long_scan: general.read_range("TiberiumLongScan", 0x2000),
            buildup_time: general.read_double("BuildupTime", f64::from_bits(0x3FA9_9999_9999_999A)),
            slave_miner_short_scan: general.read_range("SlaveMinerShortScan", 0x500),
            slave_miner_slave_scan: general.read_range("SlaveMinerSlaveScan", 0x1000),
            // Published from the retained CombatDamage pass owner below.
            drain_money_frame_delay: defaults.drain_money_frame_delay,
            drain_money_amount: defaults.drain_money_amount,
            slave_miner_long_scan: general.read_range("SlaveMinerLongScan", 0x5000),
            slave_miner_scan_correction: general.read_range("SlaveMinerScanCorrection", 0x300),
            slave_miner_kick_frame_delay: general.read_int("SlaveMinerKickFrameDelay", 0x7FFF_FFFF),
            maximum_queued_objects: general.read_int("MaximumQueuedObjects", 5),
            building_production_queue: general.read_bool("BuildingProductionQueue", false),
            #[cfg(feature = "phobos")]
            building_type_selectable: crate::rules::ares_switch::ares_way()
                && general.read_bool("BuildingTypeSelectable", false),
            ai_fire_sale: phobos_read(|| general.read_bool("AIFireSale", true)),
            ai_fire_sale_delay: phobos_read(|| general.read_int("AIFireSaleDelay", 0)),
            ai_all_to_hunt: phobos_read(|| general.read_bool("AIAllToHunt", true)),
            gather_when_mcv_deploy: phobos_read(|| general.read_bool("GatherWhenMCVDeploy", true)),
            harvester_too_far_distance: general.read_int("HarvesterTooFarDistance", 5),
            approach_target_reset_multiplier: general.read_int("ApproachTargetResetMultiplier", 1),
            chrono_harv_too_far_distance: general.read_int("ChronoHarvTooFarDistance", 50),
            harvester_load_rate: general.read_int("HarvesterLoadRate", 2),
            harvester_dump_frames: {
                // gamemd reads HarvesterDumpRate with ReadDouble and gates on
                // `rate × 900 <= accumulator`; the accumulator is integer-stepped,
                // so the first crossing is ceil(rate × 900). Take the ceiling at
                // full double precision to match that crossing exactly (no tenths
                // rounding). Clamp to u16::MAX to keep the frame threshold in range.
                let rate = general.read_double("HarvesterDumpRate", 0.016);
                (rate * 900.0).clamp(0.0, u16::MAX as f64).ceil() as u16
            },
            chrono_delay: general.read_int("ChronoDelay", 60),
            chrono_reinf_delay: general.read_int("ChronoReinfDelay", 180),
            chrono_distance_factor: general.read_int("ChronoDistanceFactor", 48),
            chrono_trigger: general.read_bool("ChronoTrigger", true),
            chrono_minimum_delay: general.read_int("ChronoMinimumDelay", 16),
            chrono_range_minimum: general.read_int("ChronoRangeMinimum", 0),
            purifier_bonus: general.read_double_to_float("PurifierBonus", defaults.purifier_bonus),
            // An IntVector (`0x00475D70`) indexed by difficulty.
            ai_virtual_purifiers: general
                .read_int_list("AIVirtualPurifiers")
                .and_then(|values| values.get(..3)?.try_into().ok())
                .unwrap_or([4, 2, 0]),
            disabled_disguise_detection_percent: general
                .read_int_list("DisabledDisguiseDetectionPercent")
                .unwrap_or_else(|| defaults.disabled_disguise_detection_percent.clone()),
            allied_crew: defaults.allied_crew.clone(),
            soviet_crew: defaults.soviet_crew.clone(),
            third_crew: defaults.third_crew.clone(),
            technician: defaults.technician.clone(),
            engineer_infantry: defaults.engineer_infantry.clone(),
            engineer_capture_level: general
                .read_double_to_float("EngineerCaptureLevel", defaults.engineer_capture_level),
            crew_escape: crate::util::native_x87::NativeF64Bits::from_bits(
                general
                    .read_double("CrewEscape", f64::from_bits(defaults.crew_escape.bits()))
                    .to_bits(),
            ),
            refund_percent: crate::util::native_x87::NativeF64Bits::from_bits(
                general
                    .read_double(
                        "RefundPercent",
                        f64::from_bits(defaults.refund_percent.bits()),
                    )
                    .to_bits(),
            ),
            // Saturating: an out-of-range literal scans as infinity.
            ship_sinking_weight: SimFixed::saturating_from_num(
                general.read_double("ShipSinkingWeight", defaults.ship_sinking_weight.to_num())
                    as f32,
            ),
            tracked_uphill: general.read_double_bits("TrackedUphill", defaults.tracked_uphill),
            tracked_downhill: general
                .read_double_bits("TrackedDownhill", defaults.tracked_downhill),
            wheeled_uphill: general.read_double_bits("WheeledUphill", defaults.wheeled_uphill),
            wheeled_downhill: general
                .read_double_bits("WheeledDownhill", defaults.wheeled_downhill),
            extra_unit_light,
            extra_infantry_light,
            extra_aircraft_light,
            close_enough: general.read_range("CloseEnough", defaults.close_enough),
            stray: general.read_range("Stray", defaults.stray),
            relaxed_stray: general.read_range("RelaxedStray", defaults.relaxed_stray),
            guard_mode_stray: general.read_range("GuardModeStray", defaults.guard_mode_stray),
            // Repair service scalars below are projected from the ordered
            // native reader; do not reread them from this flattened INI.
            repair_percent: general.read_double("RepairPercent", defaults.repair_percent),
            #[cfg(feature = "phobos")]
            prone_speed_crawls: general
                .read_phobos_double("ProneSpeed.Crawls", defaults.prone_speed_crawls),
            #[cfg(feature = "phobos")]
            prone_speed_no_crawls: general
                .read_phobos_double("ProneSpeed.NoCrawls", defaults.prone_speed_no_crawls),
            repair_step: general.read_int("RepairStep", defaults.repair_step),
            unit_repair_rate: defaults.unit_repair_rate,
            charge_to_drain_ratio: defaults.charge_to_drain_ratio,
            infantry_repair_step: defaults.infantry_repair_step,
            infantry_repair_rate: defaults.infantry_repair_rate,
            reload_rate: general.read_double("ReloadRate", defaults.reload_rate),
            // Only Ares reads these back; Rules read the stock way keep the
            // constructor's values.
            tiberium_heal_minutes: if crate::rules::ares_switch::ares_way() {
                general.read_double("TiberiumHeal", defaults.tiberium_heal_minutes)
            } else {
                defaults.tiberium_heal_minutes
            },
            tiberium_transmogrify: if crate::rules::ares_switch::ares_way() {
                general.read_int("TiberiumTransmogrify", defaults.tiberium_transmogrify)
            } else {
                defaults.tiberium_transmogrify
            },
            // The ordered Process owner publishes them (`RulesTypeReferences`).
            small_visceroid: None,
            large_visceroid: None,
            path_delay,
            blockage_path_delay_ticks,
            power_surplus,
            enable_power_surplus,
            power_surplus_scale_to_drain_amount,
            // ReadGeneral670235..670267 uses the same475D70 signed vector
            // reader as the existing Recalc difficulty tables below.
            ai_auto_deploy_frame_delay: general
                .read_int_list("AIAutoDeployFrameDelay")
                .unwrap_or_default(),
            #[cfg(feature = "phobos")]
            infantry_auto_deploy: general.read_bool("InfantryAutoDeploy", false),
            #[cfg(feature = "phobos")]
            allow_damage_on_self: general.read_bool("AllowDamageOnSelf", false),
            #[cfg(feature = "phobos")]
            decloak_damaged_targets: general.read_bool("DecloakDamagedTargets", true),
            #[cfg(feature = "phobos")]
            rof_random_delay: crate::rules::ares_switch::not_ares_read(|| {
                combat_damage.read_ares_partial_int_pair(
                    "ROF.RandomDelay",
                    crate::rules::ini_value::PartialIntPair {
                        low: 0,
                        high: 2,
                        values: 2,
                    },
                )
            }),
            // Ares 3.0 Buf_AI (1005B830, hook 0x672B0E): ParseList<int>
            // (1005B030) replaces the original IntVector read.
            ai_force_prediction_fudge: if crate::rules::ares_switch::ares_way() {
                crate::rules::ares_switch::not_ares_read(|| {
                    ai.read_ares_int_list("AIForcePredictionFudge")
                })
            } else {
                ai.read_int_list("AIForcePredictionFudge")
            }
            .unwrap_or_default(),
            ai_pick_wall_defense_percent: general
                .read_int_list("AIPickWallDefensePercent")
                .unwrap_or_default(),
            // PlayerScatter belongs to the [CombatDamage] read, IQ Scatter to
            // the [IQ] read; neither is a [General] key.
            player_scatter: combat_damage.read_bool("PlayerScatter", defaults.player_scatter),
            player_return_fire: combat_damage
                .read_bool("PlayerReturnFire", defaults.player_return_fire),
            iq_scatter: iq.read_int("Scatter", defaults.iq_scatter),
            max_iq_levels: iq.read_int("MaxIQLevels", defaults.max_iq_levels),
            iq_production,
            iq_harvester,
            iq_super_weapons,
            iq_guard_area: iq.read_int("GuardArea", defaults.iq_guard_area),
            iq_repair_sell: iq.read_int("RepairSell", defaults.iq_repair_sell),
            iq_auto_crush: iq.read_int("AutoCrush", defaults.iq_auto_crush),
            iq_sell_back: iq.read_int("SellBack", defaults.iq_sell_back),
            credit_reserve: ai.read_int("CreditReserve", defaults.credit_reserve),
            cliff_back_impassability: read_cliff_back_impassability(Some(general)),
            lightning_storm_duration: general
                .read_int("LightningStormDuration", defaults.lightning_storm_duration),
            lightning_damage: general.read_int("LightningDamage", defaults.lightning_damage),
            lightning_deferment: general
                .read_int("LightningDeferment", defaults.lightning_deferment),
            lightning_hit_delay: general
                .read_int("LightningHitDelay", defaults.lightning_hit_delay),
            lightning_scatter_delay: general
                .read_int("LightningScatterDelay", defaults.lightning_scatter_delay),
            lightning_cell_spread: general
                .read_int("LightningCellSpread", defaults.lightning_cell_spread),
            lightning_separation: general
                .read_int("LightningSeparation", defaults.lightning_separation),
            // The sole ordered AV reader supplies these after projection.
            #[cfg(feature = "phobos")]
            timer_blink_color_scheme: defaults.timer_blink_color_scheme,
            ambient_change_rate_nonzero: defaults.ambient_change_rate_nonzero,
            ambient_change_interval_frames: defaults.ambient_change_interval_frames,
            ambient_change_step: defaults.ambient_change_step,
            iron_curtain_duration: combat_damage.read_int("IronCurtainDuration", 0),
            iron_curtain_invoke_anim: defaults.iron_curtain_invoke_anim.clone(),
            chrono_placement_anim: defaults.chrono_placement_anim.clone(),
            chrono_blast_anim: defaults.chrono_blast_anim.clone(),
            chrono_blast_dest_anim: defaults.chrono_blast_dest_anim.clone(),
            dominator_warhead: defaults.dominator_warhead.clone(),
            dominator_first_anim: defaults.dominator_first_anim.clone(),
            dominator_second_anim: defaults.dominator_second_anim.clone(),
            dominator_fire_at_percentage: general.read_int("DominatorFireAtPercentage", 50),
            dominator_capture_range: general.read_int("DominatorCaptureRange", 2),
            dominator_damage: general.read_int("DominatorDamage", 50),
            // The native Process state supplies all four IonBlast inputs.
            ion_blast_anim: defaults.ion_blast_anim.clone(),
            ion_beam_anim: defaults.ion_beam_anim.clone(),
            ion_cannon_damage: defaults.ion_cannon_damage,
            spotlight: defaults.spotlight,
            force_shield_radius: general.read_int("ForceShieldRadius", 10),
            force_shield_duration: general.read_int("ForceShieldDuration", 400),
            force_shield_blackout_duration: general.read_int("ForceShieldBlackoutDuration", 800),
            force_shield_fade_sound_time: general.read_int("ForceShieldPlayFadeSoundTime", 50),
            force_shield_invoke_anim: defaults.force_shield_invoke_anim.clone(),
            psychic_reveal_radius,
            // The constructor's nulls; only the ordered Process owner reads
            // them (`RulesTypeReferences`).
            mutate_warhead: String::new(),
            mutate_explosion_warhead: String::new(),
            empulse_warhead: None,
            mutate_explosion: general.read_bool("MutateExplosion", false),
            // The processed RulesClass vector is authoritative; a merged INI
            // cannot reproduce successful-read replacement or factory identity.
            metallic_debris: Vec::new(),
            weather_con_clouds: Vec::new(),
            weather_con_bolts: Vec::new(),
            // Read only by the ordered Process owner (`native_processing`),
            // which retains them across passes; the RuleSet projection
            // publishes its values over these constructor placeholders.
            detail: defaults.detail,
            difficulty_rows: defaults.difficulty_rows,
            patrol_scan_minutes: defaults.patrol_scan_minutes,
            gravity: defaults.gravity,
            wind_direction: defaults.wind_direction,
            missile_rot_var: defaults.missile_rot_var,
            safety_altitude: defaults.safety_altitude,
            line_trail_color_override: defaults.line_trail_color_override,
            chrono_beam_color: defaults.chrono_beam_color,
            force_shield_color: defaults.force_shield_color,
            laser_target_color: defaults.laser_target_color,
            #[cfg(feature = "phobos")]
            airstrike_line_color,
            #[cfg(feature = "phobos")]
            airstrike_line_z_adjust,
            iron_curtain_color: defaults.iron_curtain_color,
            berserk_color: defaults.berserk_color,
            parachute_max_fall_rate: defaults.parachute_max_fall_rate,
            no_parachute_max_fall_rate: defaults.no_parachute_max_fall_rate,
            maximum_cheer_rate: defaults.maximum_cheer_rate,
            allied_survivor_divisor: defaults.allied_survivor_divisor,
            soviet_survivor_divisor: defaults.soviet_survivor_divisor,
            third_survivor_divisor: defaults.third_survivor_divisor,
            prism_type: defaults.prism_type,
            prerequisite_proc_alternate: defaults.prerequisite_proc_alternate,
            prism_support: defaults.prism_support,
            building_types: defaults.building_types,
            lightning_warhead: defaults.lightning_warhead,
            weather_con_bolt_explosion: defaults.weather_con_bolt_explosion,
            weapon_nullify_anim: defaults.weapon_nullify_anim,
            // The compatibility projection drops keys no native reader takes,
            // so the Phobos read reaches `GeneralRules` through the retained
            // Rules scalar instead (`from_processed_rules`).
            #[cfg(feature = "phobos")]
            missing_cameo: defaults.missing_cameo.clone(),
        }
    }
}
