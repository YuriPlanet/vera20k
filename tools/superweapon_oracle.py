"""Native references for the superweapon chains: the nuclear missile's launch
and the computer houses' use of their superweapons.

Run python -m tools.superweapon_oracle --check (or explicit --write).
Rust consumers: src/sim/superweapon/fire_tests.rs (click_fire,
defense_alert), src/sim/world/techno_ai/building_missile.rs (mission_missile),
src/sim/projectile/launch.rs (both velocities), src/sim/combat/nuke_maker_tests.rs
(nuke_maker), src/sim/building_art_super.rs (super_anim, opening_super_anim)
src/sim/superweapon/ai_fire_tests.rs (the ai_* sections),
src/sim/superweapon/chronosphere_tests.rs (the chrono_* sections) and
src/sim/superweapon/psychic_dominator_tests.rs (psydom_*, update_lighting,
ambient_step), src/sim/superweapon/spy_plane_tests.rs (spy_plane_launch,
send_spy_planes, spyplane_missions), src/sim/aircraft/leave_map_tests.rs
(aircraft_leave_map), src/sim/team_script_vm/super_actions_tests.rs
(team_super_actions) and src/sim/superweapon/invulnerability_tests.rs
(iron_tint, effect_tint_intensity), src/app/presentation/instances/units.rs
(curtain_draw_arm), src/app/presentation/curtain_tint_tests.rs
(drawshp_curtain_arm, draw_curtain_arm, building_colour_word,
anim_colour_word, building_anim_light, blit_pickers, blitters),
src/app/presentation/lighting.rs (aircraft_light) and
src/sim/superweapon/nuke_tests.rs (nuke_impact, nuke_wait, nuke_flash,
nuke_lighting_read, nuke_launch) and src/sim/superweapon/force_shield_tests.rs
(force_shield_launch, super_fade) and src/sim/superweapon/lightning_storm_tests.rs
(storm_start, storm_cloud, storm_pixel_heights, storm_strike, storm_process,
radar_outage) and src/sim/superweapon/iron_curtain_tests.rs
(iron_curtain_launch, curtain_overrides) and src/sim/superweapon/paradrop_tests.rs
(paradrop_launch, send_paradrop_planes, paradrop_missions, drop_payload,
spawn_parachuted) and src/sim/superweapon/genetic_converter_tests.rs
(genetic_launch, infantry_mutate_death, make_infantry) and
src/sim/superweapon/psychic_reveal_tests.rs (psychic_launch) and
src/app/presentation/super_timers.rs (tactical_timers, timer_lines) and
src/render/ion_blast_ripple_tests.rs (ion_blast_ripple),
src/sim/world/ion_blast_tests.rs (ion_blast_update) and
src/app/presentation/ion_blasts_tests.rs and ion_blasts_gpu_tests.rs
(ion_blast_draw).

Sections, each case in a fresh emulator (tools.ai_base_building_oracle's
fixture machinery):
- click_fire: SuperClass::ClickFire 0x6CB920 without charge drain or a
  one-time grant: admission, the Lightning Storm deferment refusal, the
  Psychic Dominator's refusal while one is active, the Launch call,
  readiness and the recharge timer writes.
- defense_alert: the computer house's launch alert 0x4FAF00 (Fire_SW's
  house loop): its gates, CoordStruct::Distance3D 0x41C380 of base minus
  cell, the Scenario RandomRanged(0, 99) draw and the stores.
- mission_missile: BuildingClass::Mission_Missile 0x44C980 in each status of
  a NukeSilo building (and a building that is not one): Begin_Mode modes,
  the PSIWARN and take-off anims, the bullet's construction and launch
  (coordinate and velocity bits, from the table sine/cosine 0x4CACB0 and
  0x4CAD00) and each returned delay.
- nuke_maker: BulletClass::NukeMaker 0x46B310: the NukePayload bullet's
  construction, launch coordinate and velocity bits.
- super_anim: BuildingClass::UpdateAnimation's SuperAnim block
  0x450F9E..0x451145 (GetHealthPercentage 0x5F5C60 and the slot clear
  0x451E40 run natively).
- opening_super_anim: OnConstructionComplete's first-opening block
  0x4463F0..0x446580.
- ai_try_fire: HouseClass::AI_TryFireSW 0x5098F0: the human gate, the
  Supers loop and its Type= jump table 0x509AE8, the MultiMissile arm, the
  Lightning Storm arm 0x509E00 (its storm gate 0x53A100) and the Force Shield
  arm; the other pickers and Fire_SW are recorded stubs.
- ai_best_rally_target: HouseClass::AI_FindBestRallyTarget 0x50CBF0 over a
  fixture TechnoClass::Array: candidates, values by kind and difficulty, the
  playfield test 0x578460, the cloak draws and the final pick, with the
  Scenario Random run natively (seeded by 0x65C6D0).
- ai_ground_rally_point: HouseClass::AI_GroundRallyPoint 0x509CD0: the base
  cell, Find_Nearby_Passable_Cell's arguments (a recorded stub) and the
  fired cell.
- ai_genetic_mutator: HouseClass::AI_Fire_GenMutator 0x509F60 over a fixture
  InfantryClass::Array and cell lists (CellClass::GetInfantry 0x47EC40 runs
  natively). Its cell-offset table 0xABD490 lies in BSS: the static
  initializer 0x561910 fills it first.
- ai_psydom: HouseClass::AI_Fire_PsyDom 0x50A150 over a fixture
  FootClass::Array and cell lists: its gates, each ground list read from its
  head while the objects are Feet, the house tests, CanBePermaMindControlled
  0x53C450 and Is_Cell_In_Playfield 0x578460 (both run natively), the sweep
  over table entries 0..=37 and the pick.
- chrono_process: a Chrono Warp's TeleportLocomotionClass from the first
  frame after Launch case 4 to the end of its piggyback: Process 0x7192F0,
  TimerCheck 0x719BF0, Is_Ok_To_End 0x719F30 and the constructor, Link and
  Begin_Piggyback run natively; the driver follows UnitClass::AI's prologue
  (0x7362A7..0x73635A) and FootClass::AI (0x4DA877, 0x4DAE5F..0x4DAEC3).
- chrono_update_position: TeleportLocomotionClass::Update_Position 0x718260
  over fixture cells and objects: the placement (Marked, floor height,
  bridge height and OnBridge), the destination cell's kills and blocks, and
  the blocked retarget (its zone and nearby-cell arguments and the new
  +0x288).
- chrono_destination: Launch case 4's +0x288 for one object of the source
  block, the Unit's and the others' arithmetic run as slices of Launch
  (0x6CC9AF..0x6CCA4C, 0x6CCB6A..0x6CCC2D).
- psydom_process: PsychicDominator::Process 0x53AF40, one step of each
  status, and status 2's first firing stage for every percent 0..100 (and a
  few outside) and anim frame count 1..64.
- psydom_start: PsyDom::Start 0x53AE50: its globals, the first anim's
  constructor arguments, Timer_1248 and the UpdateLighting call.
- update_lighting: ScenarioClass::UpdateLighting 0x53C280 for every
  NukeFlash, ChronoScreen, storm and Dominator state: the ambient target and
  RecalcLighting's arguments.
- ambient_step: LogicClass::PerTickUpdate's ambient fade
  (0x55B33D..0x55B4D7) run as a slice: gates, the interval each lighting
  state picks for Timer_1248, the target clamp and the clamped step.
- dominator_lighting_read: ScenarioClass::Set_Defaults's Dominator lighting
  values and Read_INI_Basic's conversion of each Dominator key, for a
  missing key and authored tokens.
- relight: CellClass::ProcessColourComponents 0x484180's Ground/Level arms
  (0x48445F..0x4845A2) for each storm, Dominator and NukeFlash state: the
  Dominator's top scalar reads NukeLevel (+0x3574), its bottom DominatorLevel.
- spy_plane_launch: Launch 0x6CC390 from its entry for a Type= 8 Super, case
  8 (0x6CD66F..0x6CD70B): the charge gate, the SPYP lookup, the cell and its
  dummy, the AllyParaDrop length test and loop, and the player's EVA tail.
- send_spy_planes: HouseClass::SendSpyPlanes 0x65EAB0 with case 8's
  arguments: the ScenarioInit bracket, the mission-only byte, the edge
  (+0x1E0, else GetEdge 0x50DA80, run natively), the call order, the Unlimbo
  coordinate and the failure paths.
- spyplane_missions: Mission_SpyplaneApproach 0x4155F0 and
  Mission_SpyplaneOverfly 0x4157C0 with ReReveal 0x70B1D0, UpdateReveal
  0x70AF50 and GetOppositeEdge 0x50DAC0 run natively: the branches by Target,
  NavCom and distance, the reveal radius and latch, the sound, the queued
  missions, +0x6D2, the edge cell (and the empty cell) and the frames.
- aircraft_leave_map: AircraftClass::AI's removal block 0x414F47..0x414FDF as
  a slice, with GetMapCoords 0x41BEA0, IsCellInPlayfield 0x578460, In_Bounds
  0x568300, the predicate 0x41B890 and Get_Mission 0x5B3040 run natively.
- team_super_actions: TeamClass::AI's script actions 55 (0x6EFC70, Iron
  Curtain) and 57 (0x6F0130, Chronosphere): the leader loop with the live
  test 0x6EF9E0, the Supers search (first of Type= 1; last of 3 and of 4),
  GetPowerRatio 0x4FCE30, the RechargeTimer read, GetRechargeTime 0x6CC260,
  the wait test and Quarry_To_Threat 0x645BB0, with the threat scan,
  Fire_SW and Assign_Mission_Target recorded.
- iron_tint: TechnoClass::UpdateIronTint 0x70E5A0 once a frame over a
  curtain's life, after TechnoClass::IronCurtain 0x70E2B0: the stage
  (+0x1A4), its timer (+0x198) and the Scenario draw, with
  IsIronCurtained 0x41BF40, CDTimerClass::Remaining 0x4B4D70 and
  RandomRanged 0x65C7E0 run natively.
- effect_tint_intensity: TechnoClass::GetEffectTintIntensity 0x70E360 over
  every tint stage and time left UpdateIronTint writes, applied to a set of
  draw intensities (one emulator: the function only reads).
- curtain_draw_arm: UnitClass::DrawVoxelBody's intensity and colour block
  0x73BF7B..0x73C166 as a slice: the flash arm 0x70D190, IsIronCurtained
  0x41BF40, GetEffectTintIntensity, the [ColorAdd] conversion by pixel
  format, Berzerk and the Deactivated halving 0x70FBD0 run natively.
- drawshp_curtain_arm: TechnoClass::DrawSHP's intensity arm
  0x70631F..0x706389 as a slice for a building and a unit: each class's
  flash arm, IsIronCurtained 0x41BF40, the airstrike gate and
  ScaleByIronTintPhase 0x70E380 and the airstrike phase 0x70E4B0 run
  natively.
- draw_curtain_arm: the same rows through TechnoClass::Draw's arm
  0x706776..0x7067E4 (the voxel draw) for an aircraft and a building.
- aircraft_light: AircraftClass::Draw_It's intensity 0x4148D1..0x41493E as a
  slice after the aircraft unit's static initializers (the level step
  0x889EC8 under both control words): IsActive 0x53A100, the IDIV by two
  level steps, Level=/IonLevel=, GetCellAt 0x565730 and the cell's signed
  word, and ExtraAircraftLight= run natively; GetHeight vt+0x1C8 answers
  the row.
- building_colour_word: the colour word BuildingClass_DrawBody
  (0x43D386..0x43D544) and BuildingClass::Draw (0x43DC1C..0x43DDF1) compute
  for their blits: the LaserTargetColor= and ForceShieldColor= [ColorAdd]
  colours by pixel format, the Force Shield byte, and the shroud's zero.
- anim_colour_word: AnimClass::DrawIt's colour block 0x4233EE..0x423630: the
  slot byte, the cell's first building (0x47C520, natively) and its word.
- building_anim_light: BuildingClass::UpdateAnimation's anim light
  0x450A47..0x450A77: 0x456FB0 and the slot loop 0x451F60 natively.
- blit_pickers: ConvertClass's blitter pickers 0x490B90 (uncompressed
  frames) and 0x490E50 (compressed) for each draw's flags.
- blitters: the four blitters those picks reach, each plain and tinted copy
  on a line with a hole and a Z-rejected pixel, for several colour words.
- nuke_impact: BulletClass::AI's NUKE block and tail 0x467E53..0x467FEE as a
  slice: the warhead test 0x410A40, GetHeight 0x5F5F40 and SetHeight
  0x5F5FA0 (floor 0x578080 and Mark recorded), GetMapCoords 0x41BEA0,
  FindIndex 0x427CB0 over fixture AnimTypes and 0x48ACE0 run natively;
  ScreenNukeFlash, CreateRadarEvent, the anim constructor and the handoff
  0x468D80 are recorded. The holder list 0xB0F5B8 and the committed cell.
- nuke_wait: BulletClass::AI's head 0x4666F2..0x466788 as a slice: the
  IsAlive and wait tests, the holder list's removal, the handoff and UnInit.
- nuke_flash: ScreenNukeFlash 0x53AB70 and the head of
  LightningStorm::Process (0x53A6C0..0x53A742) once a frame: the flash's
  status and timer (0xA9FABC, 0x827FC8/0x827FCC), Timer_1248, the ambient
  target, RecalcLighting's arguments and UpdateLighting.
- nuke_lighting_read: Read_INI_Basic's NukeAmbientChangeRate=
  (0x68AAD5..0x68AAFD): Set_Defaults' value, ReadDouble's default and the
  ftol of authored tokens.
- force_shield_launch: Launch 0x6CC390 from its entry for a Type= 10 Super,
  case 10 (0x6CD072..0x6CD2EB): the charge gate, the deck coordinate, the
  invoke anim's arguments, the fade countdown and coordinate (+0x50, +0x54),
  StartSound, the blackout, the walk's sentinels and the BuildingClass::Array
  walk (IsAlliedWith 0x4F9A50, CoordStruct 0x41C230 and Distance3D 0x41C380
  run natively) with each IronCurtain call, and the player's tail.
- super_fade: SuperClass::AI 0x6CBCA0's head (0x6CBCA8..0x6CBCD4) called once
  a frame: the countdown, the frame SpecialSound plays and where.
- storm_start: LightningStorm::Start 0x539EB0: the retarget, the
  countdown's minimum and duration, the empty cell's draws over MapRect
  (MapClass+0x12C/+0x130) until In_Bounds 0x568300 holds the cell, and the
  start: radar
  event 13, each house's CreateRadarOutage 0x50BCD0 (IsAlliedWith 0x4F9A50
  run natively) unless the owner counts it an ally, it is defeated (+0x1F5)
  or 0xA8B538 is set, PlayerPtr+0x5779, UpdateLighting, and StormSound and
  the message under LightningPrintText.
- storm_cloud: CreateCloudBolt 0x53A140: the cloud's coordinate (level,
  bridge bit and the first bolt image's half height through 0x6D2120), the
  Scenario draw that picks the cloud, the anim and both lists.
- storm_pixel_heights: 0x6D2120 over sample pixel counts after its
  initializers, and over every half SHP height (-16384..=16383) as a digest
  checked against the exact product under three control words.
- storm_strike: GroundStrike 0x53A300: the bolt (Get_Center_Coords 0x480A30
  natively over a fixture floor) and its draw, the strike coordinate, the
  LightningSounds draw and PlayAt, the explosion (its selector 0x48A4F0
  recorded, 0x48ACE0 natively), the flash and Apply_area_damage (recorded),
  and the debris over land, building, nearest object and level before and
  after the damage, with its draws.
- storm_process: LightningStorm::Process 0x53A6C0 once (the nuke flash
  idle): the three lists' upkeep and strikes, the end, the countdown with
  its 225-frame line and its Start, the raging cadences and the scatter's
  three tries, with CreateCloudBolt, GroundStrike and Start run natively.
- radar_outage: HouseClass::Update's outage block 0x4F8490..0x4F84D9 as a
  slice, and 0x508DF0's timer test with FreeRadar set.
- iron_curtain_launch: Launch 0x6CC390 from its entry for a Type= 1 Super,
  case 1 (0x6CCE64..0x6CD06F): the charge gate, the deck coordinate and the
  invoke anim, the EVA line and its mute byte 0xA8B538, radar event 13, the
  3x3 walk over the offset table 0xB0C038 (each cell's +0x140 choosing its
  bridge or ground list, the Foot latch skip, +0x30 read after each call)
  with each IronCurtain call, and the player's tail.
- curtain_overrides: InfantryClass::IronCurtain 0x522600 and
  FootClass::IronCurtain 0x4DEAE0: the damage call and its arguments, the
  Organic test, the parasite's release and timer, Foot+0x6A0 and the
  TechnoClass::IronCurtain call.
- paradrop_launch: Launch 0x6CC390 from its entry for a Type= 5 or 6 Super,
  cases 5 (0x6CD2EE..0x6CD534) and 6 (0x6CD537..0x6CD66A): the charge gate,
  the PDPLANE lookup, the cell and its dummy, the water test 0x485060 (run
  natively) and Find_Nearby_Passable_Cell's arguments and answer, the side's
  lists with their length tests (Soviet's absent) and loops, and the
  player's tail.
- send_paradrop_planes: HouseClass::SendParadropPlanes 0x65E660 with case
  5's arguments: the ScenarioInit bracket, the mission-only byte, the edge
  (+0x1E0, else GetEdge 0x50DA80, natively), the call order, the Unlimbo
  coordinate, the failure paths, +0x6C9, and each infantry's construction,
  Limbo and AddPassenger 0x4733A0 (natively).
- paradrop_missions: Mission_ParadropApproach 0x4158E0 and
  Mission_ParadropOverfly 0x415960: the branches by Target, NavCom,
  passengers and distance to ParadropRadius, the pass counter +0x6D3, the
  latch +0x6D2, the playfield test and the returned frames.
- drop_payload: AircraftClass::Drop_Payload 0x415C60 with
  RemoveFirstPassenger 0x473430, AddPassenger, the table trigonometry and
  ftol run natively: Ammo +0x2FC and the side it picks, the admission calls
  (Can_Enter_Cell, 0x481180, SpawnParachuted) and their arguments, ChuteSound,
  +0x55C, the team removal, +0x6D3, the rearm timer, and each failure's
  restore.
- spawn_parachuted: InfantryClass::SpawnParachuted 0x521760 with
  IsControlledByHuman 0x50B730 run natively: Paradrop's answer, the mission
  it queues and the Do_Action.
- genetic_launch: Launch 0x6CC390 from its entry for a Type= 9 Super, case 9
  (0x6CD7E7..0x6CDA62): the charge gate, the deck coordinate and the IonBlast
  anim 5 leptons over it, the EVA line and its mute byte, PlayAtCoord
  0x750E20 of GeneticMutatorActivateSound, radar event 13, then with
  MutateExplosion= Apply_area_damage 0x489280's arguments, else the 3x3 walk
  (each cell's +0x140 choosing its bridge or ground list, +0x30 read before
  each call) giving each Infantry WhatAmI its type's Strength through
  ReceiveDamage, and the player's tail.
- infantry_mutate_death: InfantryClass::ReceiveDamage's InfDeath 9 arm
  0x5188AE..0x518B2C as a slice: the Unmark/Mark of the infantryman's
  occupancy, the building scan of the cell's ground list, the Ground table's
  Foot cost against OnBridge, 0x481180's spot against the empty coordinate
  0xA8F200, the Die2 fallback, and the InfantryMutate anim's arguments, house
  (SetOwnerHouse 0x424CA0, natively), remap (ColorScheme +0x30C) and
  MarkCellOccupancy.
- make_infantry: AnimClass::AI's MakeInfantry block 0x424932..0x424B30 as a
  slice: ClearCellOccupancy, the MakeInfantry-against-count bound, the
  Civilian house fallback over HouseClass::Array, CreateObject's house,
  Unlimbo's coordinate and facing and its refused retry (the stage stepped
  back), the bridge lift (Mark up, OnBridge, Mark down), Hunt for a house
  that is not human, and the UnInit.
- psychic_launch: Launch 0x6CC390 from its entry for a Type= 11 Super, case
  11 (0x6CD70C..0x6CD7E7): the charge gate, the cell's coordinate, both
  MapClass::RevealArea 0x5678E0 calls' arguments, PlayAtCoord 0x750E20 of
  PsychicRevealActivateSound and the player's tail.
- nuke_launch: Launch 0x6CC390 from its entry for a Type= 0 Super that is
  not one-time, case 0 (0x6CDA67, 0x6CDCF0..0x6CDE36): the charge gate, the
  silo type scan and Find_Building_Of_Type 0x4FD060, the Missile mission,
  the house's cell (+0x5784), the building's firing type (+0x5F8), DigSound
  at the floor coordinate, EVA_NuclearMissileLaunched, the player's tail and
  the house's recheck flag (+0x1FC).
- tactical_timers: TacticalClass::Draw's timer block (0x6D4941..0x6D4B25)
  as a slice: the Scenario timer's line, each listed Super's (0xA83D50 in
  list order, with the GameMode 0 hold skip through GetRechargeTime
  0x6CC260), then each house's blackout line: line index, scheme, seconds
  and label of every 0x6D4B50 call.
- timer_lines: 0x6D4B50 for one line through Fancy_Text_Print_Wide
  0x4A61C0 and its print 0x4A5EB0: the texts, measures, right alignment,
  black boxes, packed colours and the blink.
- ion_blast_ripple: the ring step's static initializers 0x53CA30/0x53CA50
  under the startup and the process control words, the ripple generator
  0x53D330 (each frame's digest, written count and byte values) and the
  spiral functions 0x53D8E0 and 0x53D960.
- ion_blast_update: IonBlastClass::UpdateAll 0x53D310 over forceless blasts:
  the frames left and the removals.
- ion_blast_draw: IonBlastClass::DrawAll 0x53D850 and the per-blast draw
  0x53D580 on fixture surfaces: CoordsToClient2's points, the clipped
  ripple, the Z threshold, the two-row step and the draw order.
"""
from pathlib import Path
import math
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX,
                               UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_EDX,
                               UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW)

from tools.ai_base_building_oracle import FAKE, RULES, SCENARIO, STUBS, Emu, u32
from tools.native_oracle import (NATIVE_FPCW, RET_MAGIC, STACK_BASE, STACK_SIZE, OracleError,
                                 finish_vectors, provenance, run_checked)

FRAME = 0xA8ED84
SW_TYPES = 0xA8E334
ANIM_TYPES = 0x8B4154
WEAPON_TYPES = 0x88756C
SCENARIO_PTR = 0xA8B230

# Fixture objects past tools.ai_base_building_oracle's regions.
BASE = FAKE + 0x400000
SUPER = BASE
SW_TYPE = BASE + 0x1000
HOUSE = BASE + 0x2000
HOUSE_TYPE = BASE + 0x20000
BUILDING = BASE + 0x21000
BUILDING_TYPE = BASE + 0x22000
BUILDING_VT = BASE + 0x24000
CELL = BASE + 0x25000
CELL_VT = BASE + 0x26000
BULLET = BASE + 0x27000
BULLET_VT = BASE + 0x28000
WEAPON = BASE + 0x29000
BULLET_TYPE = BASE + 0x2A000
WARHEAD = BASE + 0x2B000
ITEMS = BASE + 0x2C000
ANIM_TYPE_PSIWARN = BASE + 0x2D000
ANIM_TYPE_TAKEOFF = BASE + 0x2E000
YARD = BASE + 0x2F000
YARD_VT = BASE + 0x30000
TARGET = BASE + 0x31000
TARGET_VT = BASE + 0x32000
UP_WEAPON = BASE + 0x33000
UP_BULLET_TYPE = BASE + 0x34000
ANIM_VT = BASE + 0x35000
SLOT_ANIMS = BASE + 0x36000
NAMES = BASE + 0x38000
CELL_ARG = BASE + 0x3F000

STUB_LAUNCH = 0x6CC390
STUB_B_COORDS = STUBS + 0x100
STUB_B_FLH = STUBS + 0x110
STUB_QUEUE = STUBS + 0x120
STUB_C_COORDS = STUBS + 0x130
STUB_LIMBO = STUBS + 0x140
STUB_FIRE = STUBS + 0x150
STUB_DELETE = STUBS + 0x160
STUB_Y_COORDS = STUBS + 0x170
STUB_T_COORDS = STUBS + 0x180
STUB_MISSION = STUBS + 0x190
STUB_TYPE = STUBS + 0x1A0
STUB_OCCUPANTS = STUBS + 0x1B0
STUB_ANIM_DELETE = STUBS + 0x1C0

LEVEL_LEPTONS = 104
# The C++ initializers run from _cinit 0x7CBDAF after _setdefaultprecision
# (53-bit), before WinMain sets truncation at 0x6BBFC1: the startup word.
STARTUP_FPCW = 0x027F


def invoke_under(emu, entry, *, fpcw=NATIVE_FPCW, ecx=None, edx=None, count=5_000_000,
                 timeout_us=10_000_000):
    """`entry` with no stack arguments under `fpcw`."""
    uc = emu.uc
    sp = STACK_BASE + STACK_SIZE - 0x1000
    uc.mem_write(sp, u32(RET_MAGIC))
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_FPCW, fpcw)
    for register, value in ((UC_X86_REG_ECX, ecx), (UC_X86_REG_EDX, edx)):
        if value is not None:
            uc.reg_write(register, value & 0xFFFFFFFF)
    run_checked(uc, entry, RET_MAGIC, count=count, timeout_us=timeout_us)
    if uc.reg_read(UC_X86_REG_ESP) != sp + 4:
        raise OracleError(f'0x{entry:08X} returned with an unexpected stack')
    return uc.reg_read(UC_X86_REG_EAX)


def i32(value):
    return struct.unpack('<i', u32(value))[0]


def f32_bits(value):
    return struct.unpack('<I', struct.pack('<f', value))[0]


def f64_bits(value):
    return struct.unpack('<Q', struct.pack('<d', value))[0]


def write8(emu, address, value):
    emu.uc.mem_write(address, bytes([int(value) & 0xFF]))


def read8(emu, address):
    return emu.uc.mem_read(address, 1)[0]


def write_coord(emu, address, coord):
    emu.uc.mem_write(address, struct.pack('<iii', *coord))


def read_coord(emu, address):
    return list(struct.unpack('<iii', emu.uc.mem_read(address, 12)))


def read_cell(emu, address):
    return list(struct.unpack('<hh', emu.uc.mem_read(address, 4)))


def read_name(emu, address):
    raw = bytes(emu.uc.mem_read(address, 0x40))
    return raw.split(b'\0', 1)[0].decode('ascii')


def cell_coords(cell, levels):
    """The coordinates a flat cell's GetCoords reports: its centre, raised by
    its level (`CellClass::GetCoords` vt+0x48 is supplied)."""
    x, y = cell
    return [x * 256 + 128, y * 256 + 128, levels.get(tuple(cell), 0) * LEVEL_LEPTONS]


class Cells:
    """MapClass::operator[] 0x5657A0 answering one fixture CellClass per
    lookup, whose GetCoords (vt+0x48) reports the looked-up cell's centre
    raised by its supplied level."""

    def __init__(self, emu, levels):
        self.levels = levels
        self.cells = {}
        emu.write32(CELL_VT + 0x48, STUB_C_COORDS)
        emu.hook(0x5657A0, self.lookup, 4)
        emu.hook(STUB_C_COORDS, self.coords, 4)

    def lookup(self, emu):
        looked = read_cell(emu, emu.arg(0))
        this = CELL + 0x40 * len(self.cells)
        emu.write32(this, CELL_VT)
        self.cells[this] = looked
        emu.events.append(['cell', looked])
        return this

    def coords(self, emu):
        out = emu.arg(0)
        cell = self.cells[emu.uc.reg_read(UC_X86_REG_ECX)]
        write_coord(emu, out, cell_coords(cell, self.levels))
        return out


def coords_stub(coords):
    """vt+0x48 GetCoords(out): write `coords` to the out pointer and return it."""
    def answer(emu):
        out = emu.arg(0)
        write_coord(emu, out, coords() if callable(coords) else coords)
        return out
    return answer


# ---------------------------------------------------------------- click_fire

TYPE_MULTI_MISSILE = 0
TYPE_LIGHTNING_STORM = 2
TYPE_PSYCHIC_DOMINATOR = 7


def click_fire_row(*, kind=TYPE_MULTI_MISSILE, pre_click=False, post_click=False,
                   manual=False, recharge=900, start=-1, left=0, granted=True,
                   charged=True, on_hold=False, frame=5000, deferment=False, player=True,
                   dominator_active=False):
    emu = Emu()
    emu.write32(FRAME, frame)
    emu.write32(SUPER + 0x24, -1)
    emu.write32(SUPER + 0x28, SW_TYPE)
    emu.write32(SUPER + 0x2C, HOUSE)
    emu.write32(SUPER + 0x30, start)
    emu.write32(SUPER + 0x34, 0)
    emu.write32(SUPER + 0x38, left)
    emu.write32(SUPER + 0x68, 0)
    write8(emu, SUPER + 0x6C, 0)
    write8(emu, SUPER + 0x6D, granted)
    write8(emu, SUPER + 0x6E, 0)
    write8(emu, SUPER + 0x6F, charged)
    write8(emu, SUPER + 0x70, on_hold)
    emu.write32(SUPER + 0x78, 7)
    emu.write32(SUPER + 0x7C, 7)
    emu.write32(SW_TYPE + 0xB0, recharge)
    emu.write32(SW_TYPE + 0xB4, kind)
    write8(emu, SW_TYPE + 0xE5, 0)
    write8(emu, SW_TYPE + 0xED, pre_click)
    write8(emu, SW_TYPE + 0xEE, post_click)
    write8(emu, SW_TYPE + 0xF5, manual)
    emu.uc.mem_write(CELL_ARG, struct.pack('<hh', 33, 44))

    def launch(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != SUPER or e.arg(0) != CELL_ARG:
            raise OracleError('Launch called with an unexpected Super or cell')
        e.events.append(['launch', e.arg(1) & 0xFF])
        return 0

    def has_deferment(e):
        e.events.append(['has_deferment'])
        return int(deferment)

    def storm_message(e):
        e.events.append(['storm_message'])

    def psydom_active(e):
        e.events.append(['psychic_dominator_active'])
        return int(dominator_active)

    def psydom_message(e):
        e.events.append(['dominator_message'])

    emu.hook(STUB_LAUNCH, launch, 8)
    emu.hook(0x53A0E0, has_deferment, 0)
    emu.hook(0x53AE00, storm_message, 0)
    emu.hook(0x53B400, psydom_active, 0)
    emu.hook(0x53B410, psydom_message, 0)
    result = emu.invoke(0x6CB920, ecx=SUPER, args=[int(player), CELL_ARG]) & 0xFF
    return dict(kind=kind, pre_click=pre_click, post_click=post_click, manual=manual,
                recharge=recharge, start=start, left=left, granted=granted,
                charged=charged, on_hold=on_hold, frame=frame, deferment=deferment,
                player=player, dominator_active=dominator_active, result=result,
                events=emu.events,
                start_after=emu.read_i32(SUPER + 0x30),
                left_after=emu.read_i32(SUPER + 0x38),
                granted_after=bool(read8(emu, SUPER + 0x6D)),
                charged_after=bool(read8(emu, SUPER + 0x6F)),
                on_hold_after=bool(read8(emu, SUPER + 0x70)),
                cameo_after=emu.read_i32(SUPER + 0x78))


def click_fire():
    rows = []
    frame = 5000
    timers = [(-1, 0), (-1, 300), (frame - 900, 900), (frame - 100, 900), (frame, 0)]
    for kind in (TYPE_MULTI_MISSILE, TYPE_LIGHTNING_STORM):
        for start, left in timers:
            for granted in (False, True):
                for charged in (False, True):
                    for on_hold in (False, True):
                        rows.append(click_fire_row(kind=kind, start=start, left=left,
                                                   granted=granted, charged=charged,
                                                   on_hold=on_hold, frame=frame))
    for deferment in (False, True):
        for player in (False, True):
            rows.append(click_fire_row(kind=TYPE_LIGHTNING_STORM, start=frame - 900,
                                       left=900, deferment=deferment, player=player))
    for active in (False, True):
        for player in (False, True):
            rows.append(click_fire_row(kind=TYPE_PSYCHIC_DOMINATOR, start=frame - 900,
                                       left=900, dominator_active=active, player=player))
    for pre_click, post_click, manual in ((True, False, False), (False, True, False),
                                          (False, False, True), (True, True, False),
                                          (False, True, True)):
        for start, left in ((-1, 0), (frame - 900, 900), (frame - 20, 900)):
            for charged in (False, True):
                rows.append(click_fire_row(pre_click=pre_click, post_click=post_click,
                                           manual=manual, start=start, left=left,
                                           charged=charged, frame=frame))
    for recharge in (0, 1, 4500, 27000):
        rows.append(click_fire_row(recharge=recharge, start=frame - 7, left=3))
        rows.append(click_fire_row(recharge=recharge, manual=True, start=frame - 7, left=3))
    return rows


# ---------------------------------------------------------------- defense_alert

def defense_alert_row(*, passive=False, human=False, defend=True, cell=(40, 40),
                      base=(30, 30), alternate=(0, 0), levels=None, distance=2560,
                      difficulty=0, probability=(50, 40, 30), answer=10, yard=None,
                      frame=777):
    levels = levels or {}
    emu = Emu()
    emu.write32(FRAME, frame)
    emu.write32(HOUSE + 0x34, HOUSE_TYPE)
    write8(emu, HOUSE_TYPE + 0x1A6, passive)
    write8(emu, HOUSE + 0x1EC, human)
    emu.write32(HOUSE + 0x184, difficulty)
    emu.uc.mem_write(HOUSE + 0x5490, struct.pack('<hh', *base))
    emu.uc.mem_write(HOUSE + 0x5494, struct.pack('<hh', *alternate))
    emu.uc.mem_write(HOUSE + 0x54F4, struct.pack('<hh', -7, -7))
    emu.write32(HOUSE + 0x54FC, -100)
    emu.write32(SUPER + 0x28, SW_TYPE)
    write8(emu, SW_TYPE + 0xEC, defend)
    emu.write32(RULES + 0xEE4, distance)
    emu.write32(RULES + 0xEC8, ITEMS)
    for slot, value in enumerate(probability):
        emu.write32(ITEMS + 4 * slot, value)
    if yard is not None:
        emu.write32(HOUSE + 0x54, ITEMS + 0x100)
        emu.write32(HOUSE + 0x60, 1)
        emu.write32(ITEMS + 0x100, YARD)
        emu.write32(YARD, YARD_VT)
        emu.write32(YARD_VT + 0x48, STUB_Y_COORDS)
        emu.hook(STUB_Y_COORDS, coords_stub(yard), 4)
    else:
        emu.write32(HOUSE + 0x60, 0)
    Cells(emu, levels)
    emu.uc.mem_write(CELL_ARG, struct.pack('<hh', *cell))
    emu.draws([answer])
    emu.invoke(0x4FAF00, ecx=HOUSE, args=[SUPER, CELL_ARG])
    draws = [event for event in emu.events if event[0] == 'draw']
    return dict(passive=passive, human=human, defend=defend, cell=list(cell),
                base=list(base), alternate=list(alternate),
                levels=[[x, y, level] for (x, y), level in sorted(levels.items())],
                distance=distance, difficulty=difficulty, probability=list(probability),
                answer=answer, yard=yard, frame=frame,
                draws=[[stream, i32(low), i32(high)] for _, stream, low, high, _ in draws],
                defense_cell=read_cell(emu, HOUSE + 0x54F4),
                defense_frame=emu.read_i32(HOUSE + 0x54FC))


def defense_alert():
    rows = [defense_alert_row(passive=True), defense_alert_row(human=True),
            defense_alert_row(defend=False)]
    # Distances on both sides of the limit, along an axis and a diagonal.
    for cell in ((40, 30), (39, 30), (41, 30), (37, 37), (38, 37), (38, 38), (30, 30)):
        for distance in (2560, 2559, 2561, 0):
            rows.append(defense_alert_row(cell=cell, distance=distance))
    # Height enters the distance.
    rows.append(defense_alert_row(cell=(40, 30), levels={(40, 30): 4}))
    rows.append(defense_alert_row(cell=(39, 30), levels={(30, 30): 3}))
    rows.append(defense_alert_row(cell=(39, 30), levels={(39, 30): 25}, distance=2600))
    # The draw against each difficulty's probability.
    for difficulty in (0, 1, 2):
        for answer in (0, 29, 30, 31, 40, 50, 51, 99):
            rows.append(defense_alert_row(cell=(32, 33), difficulty=difficulty,
                                          answer=answer))
    for probability in ((0, 0, 0), (100, 100, 100), (-1, -1, -1)):
        for answer in (0, 99):
            rows.append(defense_alert_row(cell=(32, 33), probability=probability,
                                          answer=answer))
    # The base: alternate centre over primary, the origin for none.
    rows.append(defense_alert_row(cell=(32, 33), alternate=(31, 31)))
    rows.append(defense_alert_row(cell=(32, 33), alternate=(60, 60)))
    rows.append(defense_alert_row(cell=(5, 5), base=(0, 0)))
    rows.append(defense_alert_row(cell=(1, 1), base=(0, 0), distance=500))
    rows.append(defense_alert_row(cell=(1, 1), base=(0, 0), alternate=(2, 2)))
    # The defended cell: the first construction yard's.
    # Building centres: a 1x1 yard on flat and raised ground, a 4x4 one.
    for yard in ([31 * 256 + 128, 29 * 256 + 128, 0], [31 * 256 + 128, 29 * 256 + 128, 208],
                 [12 * 256 + 512, 50 * 256 + 512, 0]):
        rows.append(defense_alert_row(cell=(32, 33), yard=yard))
        rows.append(defense_alert_row(cell=(32, 33), alternate=(31, 31), yard=yard))
    return rows


# ---------------------------------------------------------------- mission_missile

BULLET_SLOT = 1  # the SuperWeaponTypes index the silo fires (+0x5F8)


def mission_missile_row(*, status, silo=True, ready=False, target=(50, 60),
                        target_level=0, origin=(20 * 256 + 128, 30 * 256 + 128, 300),
                        fire_accepts=True, bullet=True, damage=1000, frame=1234):
    emu = Emu()
    emu.write32(FRAME, frame)
    emu.write32(BUILDING, BUILDING_VT)
    emu.write32(BUILDING + 0x520, BUILDING_TYPE)
    write8(emu, BUILDING_TYPE + 0x16BA, silo)
    emu.write32(BUILDING + 0xBC, status)
    write8(emu, BUILDING + 0x6DD, ready)
    emu.write32(BUILDING + 0x21C, HOUSE)
    emu.write32(BUILDING + 0x5F8, BULLET_SLOT)
    emu.write32(BUILDING + 0x54C, 0)
    emu.uc.mem_write(HOUSE + 0x5784, struct.pack('<hh', *target))
    emu.write32(SW_TYPES, ITEMS)
    emu.write32(ITEMS + 4 * BULLET_SLOT, SW_TYPE)
    emu.write32(SW_TYPE + 0x9C, WEAPON)
    emu.write32(WEAPON + 0xA0, BULLET_TYPE)
    emu.write32(WEAPON + 0xA4, damage)
    emu.write32(WEAPON + 0xAC, WARHEAD)
    emu.write32(ANIM_TYPES, ITEMS + 0x100)
    emu.write32(ITEMS + 0x100 + 4 * 3, ANIM_TYPE_PSIWARN)
    emu.write32(RULES + 0x98, ANIM_TYPE_TAKEOFF)
    emu.write32(BUILDING_VT + 0x48, STUB_B_COORDS)
    emu.write32(BUILDING_VT + 0xB0, STUB_B_FLH)
    emu.write32(BUILDING_VT + 0x1E8, STUB_QUEUE)
    cells = Cells(emu, {tuple(target): target_level})
    emu.write32(BULLET, BULLET_VT)
    emu.write32(BULLET_VT + 0xD4, STUB_LIMBO)
    emu.write32(BULLET_VT + 0x1F0, STUB_FIRE)
    emu.write32(BULLET_VT + 0x20, STUB_DELETE)
    anims = {}

    def begin_mode(e):
        e.events.append(['begin_mode', e.arg(0)])

    def find_anim_type(e):
        e.events.append(['anim_type', read_name(e, e.uc.reg_read(UC_X86_REG_ECX))])
        return 3

    def anim_ctor(e):
        this = e.uc.reg_read(UC_X86_REG_ECX)
        kind = {ANIM_TYPE_PSIWARN: 'PSIWARN', ANIM_TYPE_TAKEOFF: 'take_off'}[e.arg(0)]
        anims[this] = kind
        e.events.append(['anim', kind, read_coord(e, e.arg(1)), i32(e.arg(2)), i32(e.arg(3)),
                         e.arg(4), i32(e.arg(5)), e.arg(6) & 0xFF])
        return this

    def anim_bullet(e):
        attached = e.arg(0)
        e.events.append(['anim_bullet', anims[e.uc.reg_read(UC_X86_REG_ECX)],
                         'bullet' if attached == BULLET else attached])

    def anim_house(e):
        e.events.append(['anim_house', anims[e.uc.reg_read(UC_X86_REG_ECX)],
                         'house' if e.arg(0) == HOUSE else e.arg(0)])

    def create_bullet(e):
        if (e.uc.reg_read(UC_X86_REG_ECX) != BULLET_TYPE or e.arg(0) != BUILDING
                or e.arg(2) != WARHEAD):
            raise OracleError('CreateBullet called with unexpected type, owner or warhead')
        target_cell = cells.cells.get(e.uc.reg_read(UC_X86_REG_EDX))
        e.events.append(['create_bullet', target_cell, i32(e.arg(1)), i32(e.arg(3)),
                         e.arg(4) & 0xFF])
        return BULLET if bullet else 0

    def set_weapon(e):
        e.events.append(['set_weapon', 'weapon' if e.arg(0) == WEAPON else e.arg(0)])

    def limbo(e):
        e.events.append(['limbo'])
        return 0

    def fire(e):
        velocity = struct.unpack('<QQQ', e.uc.mem_read(e.arg(1), 24))
        e.events.append(['fire', read_coord(e, e.arg(0)), list(velocity)])
        return int(fire_accepts)

    def delete(e):
        e.events.append(['delete_bullet', e.arg(0)])

    def flh(e):
        if e.arg(1) != 0 or read_coord(e, e.uc.reg_read(UC_X86_REG_ESP) + 12) != [0, 0, 0]:
            raise OracleError('GetFLH called with an unexpected weapon or offset')
        e.events.append(['flh'])
        write_coord(e, e.arg(0), origin)
        return e.arg(0)

    def queue(e):
        e.events.append(['queue_mission', e.arg(0), e.arg(1) & 0xFF])

    emu.hook(0x447780, begin_mode, 4)
    emu.hook(0x427CB0, find_anim_type, 0)
    emu.hook(0x421EA0, anim_ctor, 0x1C)
    emu.hook(0x424C90, anim_bullet, 4)
    emu.hook(0x424CA0, anim_house, 4)
    emu.hook(0x46B050, create_bullet, 0x14)
    emu.hook(0x46B260, set_weapon, 4)
    emu.hook(STUB_LIMBO, limbo, 0)
    emu.hook(STUB_FIRE, fire, 8)
    emu.hook(STUB_DELETE, delete, 4)
    emu.hook(STUB_B_FLH, flh, 20)
    emu.hook(STUB_B_COORDS, coords_stub([1, 2, 3]), 4)
    emu.hook(STUB_QUEUE, queue, 8)
    if not silo:
        # The other arm reaches the mission rate; supply its MissionControl
        # row (0x5B3A00) with a Rate of one minute.
        emu.write32(BUILDING + 0x5F8, -1)
        row = ITEMS + 0x400
        emu.uc.mem_write(row + 0x10, struct.pack('<d', 1.0))
        emu.hook(0x5B3A00, lambda _e: row, 0)
    delay = i32(emu.invoke(0x44C980, ecx=BUILDING))
    anim_fields = {kind: dict(hidden=read8(emu, this + 0x19D),
                              z_adjust=emu.read_i32(this + 0x100))
                   for this, kind in anims.items()}
    warning = emu.read32(BUILDING + 0x54C)
    return dict(status=status, silo=silo, ready=ready, target=list(target),
                target_level=target_level, origin=list(origin), fire_accepts=fire_accepts,
                bullet=bullet, damage=damage, frame=frame, delay=delay, events=emu.events,
                status_after=emu.read_i32(BUILDING + 0xBC),
                ready_after=read8(emu, BUILDING + 0x6DD),
                warning_kept=anims.get(warning) if warning else None,
                psiwarn_hidden=anim_fields.get('PSIWARN', {}).get('hidden'),
                take_off_z_adjust=anim_fields.get('take_off', {}).get('z_adjust'))


def mission_missile():
    rows = []
    for status in range(5):
        for ready in (False, True):
            rows.append(mission_missile_row(status=status, ready=ready))
    rows.append(mission_missile_row(status=0, target=(3, 200), target_level=6,
                                    origin=(-5, 70000, -12)))
    rows.append(mission_missile_row(status=0, fire_accepts=False))
    rows.append(mission_missile_row(status=2, fire_accepts=False))
    rows.append(mission_missile_row(status=0, bullet=False))
    rows.append(mission_missile_row(status=0, silo=False))
    return rows


# ---------------------------------------------------------------- nuke_maker

def nuke_maker_row(*, target_coords=(50 * 256 + 128, 60 * 256 + 128, 0), cell_level=0,
                   altitude=7000, payload_speed=50, payload_damage=1000):
    emu = Emu()
    up = BULLET + 0x800
    emu.write32(up + 0x10C, TARGET)
    emu.write32(up + 0xB0, BUILDING)
    emu.write32(up + 0x130, UP_WEAPON)
    emu.write32(UP_WEAPON + 0xA0, UP_BULLET_TYPE)
    emu.write32(UP_BULLET_TYPE + 0x2BC, altitude)
    emu.write32(TARGET, TARGET_VT)
    emu.write32(TARGET_VT + 0x48, STUB_T_COORDS)
    emu.hook(STUB_T_COORDS, coords_stub(list(target_coords)), 4)
    Cells(emu, {(target_coords[0] // 256, target_coords[1] // 256): cell_level})

    def find_weapon(e):
        e.events.append(['weapon_type', read_name(e, e.uc.reg_read(UC_X86_REG_ECX))])
        return 2

    emu.hook(0x773030, find_weapon, 0)
    emu.write32(WEAPON_TYPES, ITEMS)
    emu.write32(ITEMS + 8, WEAPON)
    emu.write32(WEAPON + 0xA0, BULLET_TYPE)
    emu.write32(WEAPON + 0xA4, payload_damage)
    emu.write32(WEAPON + 0xA8, payload_speed)
    emu.write32(WEAPON + 0xAC, WARHEAD)
    emu.write32(BULLET, BULLET_VT)
    emu.write32(BULLET_VT + 0xD4, STUB_LIMBO)
    emu.write32(BULLET_VT + 0x1F0, STUB_FIRE)

    def co_create(e):
        # CoCreateInstance(CLSID, outer, context, IID, out): the new bullet.
        emu.write32(e.arg(4), BULLET)
        e.events.append(['co_create'])
        return 0

    def construct(e):
        if (e.uc.reg_read(UC_X86_REG_ECX) != BULLET or e.arg(0) != BULLET_TYPE
                or e.arg(1) != TARGET or e.arg(2) != BUILDING or e.arg(4) != WARHEAD):
            raise OracleError('Construct called with unexpected type, target, owner or warhead')
        e.events.append(['construct', i32(e.arg(3)), i32(e.arg(5)), e.arg(6) & 0xFF])

    def limbo(e):
        e.events.append(['limbo'])
        return 0

    def fire(e):
        velocity = struct.unpack('<QQQ', e.uc.mem_read(e.arg(1), 24))
        e.events.append(['fire', read_coord(e, e.arg(0)), list(velocity)])
        return 1

    emu.write32(0x7E15FC, STUBS + 0x1D0)
    emu.hook(STUBS + 0x1D0, co_create, 0x14)
    emu.hook(0x4664C0, construct, 0x1C)
    emu.hook(STUB_LIMBO, limbo, 0)
    emu.hook(STUB_FIRE, fire, 8)
    emu.invoke(0x46B310, ecx=up)
    return dict(target_coords=list(target_coords), cell_level=cell_level, altitude=altitude,
                payload_speed=payload_speed, payload_damage=payload_damage,
                events=emu.events,
                payload_weapon='weapon' if emu.read32(BULLET + 0x130) == WEAPON else None)


def nuke_maker():
    return [nuke_maker_row(),
            nuke_maker_row(target_coords=(50 * 256 + 255, 60 * 256, 0)),
            nuke_maker_row(target_coords=(7 * 256 + 128, 9 * 256 + 128, 416), cell_level=4,
                           altitude=0),
            nuke_maker_row(target_coords=(12927, 15487, 0), altitude=-30),
            nuke_maker_row(payload_speed=255, payload_damage=7)]


# ---------------------------------------------------------------- super anims

SLOT_NAMES = {14: (0x1304, 0x1314, 0x1324), 15: (0x1348, 0x1358, None),
              16: (0x138C, 0x139C, 0x13AC), 17: (0x13D0, 0x13E0, None)}


def install_super_anim_fixture(emu, *, kind, cat_bits, mission, supers, slots, health,
                               strength, names, frame, yellow=0.5):
    emu.write32(FRAME, frame)
    emu.write32(BUILDING, BUILDING_VT)
    emu.write32(BUILDING + 0x520, BUILDING_TYPE)
    emu.write32(BUILDING + 0x21C, HOUSE)
    emu.write32(BUILDING + 0x6C, health)
    emu.write32(BUILDING_TYPE + 0x16F0, kind)
    emu.write32(BUILDING_TYPE + 0x16E8, cat_bits)
    emu.write32(BUILDING_TYPE + 0xA0, strength)
    emu.write32(BUILDING_VT + 0x184, STUB_MISSION)
    emu.write32(BUILDING_VT + 0x88, STUB_TYPE)
    emu.write32(BUILDING_VT + 0x408, STUB_OCCUPANTS)
    emu.hook(STUB_MISSION, lambda _e: mission, 0)
    emu.hook(STUB_TYPE, lambda _e: BUILDING_TYPE, 0)
    emu.uc.mem_write(RULES + 0x1700, struct.pack('<d', yellow))
    emu.write32(HOUSE + 0x258, ITEMS)
    emu.write32(HOUSE + 0x264, len(supers))
    for index, (super_kind, start, left) in enumerate(supers):
        this = SUPER + 0x100 * index
        sw_type = SW_TYPE + 0x100 * index
        emu.write32(ITEMS + 4 * index, this)
        emu.write32(this + 0x28, sw_type)
        emu.write32(sw_type + 0xB4, super_kind)
        emu.write32(this + 0x30, start)
        emu.write32(this + 0x38, left)
    for slot in range(0x15):
        emu.write32(BUILDING + 0x55C + 4 * slot, 0)
    for slot in slots:
        anim = SLOT_ANIMS + 0x100 * slot
        emu.write32(anim, ANIM_VT)
        emu.write32(BUILDING + 0x55C + 4 * slot, anim)
    emu.write32(ANIM_VT + 0x20, STUB_ANIM_DELETE)

    def anim_delete(e):
        e.events.append(['delete_slot', (e.uc.reg_read(UC_X86_REG_ECX) - SLOT_ANIMS) // 0x100])

    emu.hook(STUB_ANIM_DELETE, anim_delete, 4)
    for slot, offsets in SLOT_NAMES.items():
        for variant, offset in enumerate(offsets):
            if offset is None:
                continue
            text = names.get((slot, variant), '')
            emu.uc.mem_write(BUILDING_TYPE + offset, text.encode('ascii') + b'\0')

    def play(e):
        name = read_name(e, e.arg(0))
        e.events.append(['play', name, i32(e.arg(1)), e.arg(2) & 0xFF, e.arg(3) & 0xFF,
                         i32(e.arg(4))])

    emu.hook(0x451890, play, 0x14)


def default_names():
    return {(14, 0): 'SA14', (14, 1): 'SA14D', (14, 2): 'SA14G', (15, 0): 'SA15',
            (15, 1): 'SA15D', (16, 0): 'SA16', (16, 1): 'SA16D', (16, 2): 'SA16G',
            (17, 0): 'SA17', (17, 1): 'SA17D'}


def super_anim_row(*, cat=1.0, cat_bits=None, kind=0, mission=1, supers=((0, -1, 899),),
                   slots=(14, 16), health=100, strength=100, names=None, frame=9000):
    cat_bits = f32_bits(cat) if cat_bits is None else cat_bits
    names = default_names() if names is None else names
    emu = Emu()
    install_super_anim_fixture(emu, kind=kind, cat_bits=cat_bits, mission=mission,
                               supers=supers, slots=slots, health=health,
                               strength=strength, names=names, frame=frame)
    uc = emu.uc
    sp = STACK_BASE + STACK_SIZE - 0x1000
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_ESI, BUILDING)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    run_checked(uc, 0x450F9E, 0x451145, count=200_000)
    return dict(cat_bits=cat_bits, kind=kind, mission=mission,
                supers=[list(row) for row in supers], slots=list(slots), health=health,
                strength=strength, frame=frame,
                names={f'{slot}/{variant}': name for (slot, variant), name in names.items()},
                events=emu.events)


def super_anim():
    rows = []
    # The near-charged decision around ChargedAnimTime minutes.
    for cat, lefts in ((1.0, (0, 899, 900, 901, 5000)), (0.5, (449, 450, 451)),
                       (0.0, (0, 1)), (-1.0, (0, 1)), (2.25, (2024, 2025, 2026)),
                       (0.1, (89, 90, 91)), (990.0, (890999, 891000, 891001)),
                       (5.0 / 3.0, (1499, 1500, 1501))):
        for left in lefts:
            rows.append(super_anim_row(cat=cat, supers=((0, -1, left),)))
    # The gate on ChargedAnimTime itself, NaN and the infinities.
    for cat_bits in (f32_bits(990.0001), f32_bits(999.0), 0x7FC00000, 0x7F800000,
                     0xFF800000):
        rows.append(super_anim_row(cat_bits=cat_bits, supers=((0, -1, 10),)))
        rows.append(super_anim_row(cat_bits=cat_bits, supers=((0, -1, 10_000_000),)))
    # A running timer: elapsed against its duration.
    for start, left in ((9000 - 100, 999), (9000 - 100, 1000), (9000 - 100, 1001),
                        (9000 - 5000, 900), (9000, 0)):
        rows.append(super_anim_row(supers=((0, start, left),)))
    # Construction and Selling skip; the slots must be occupied.
    for mission in (0x12, 0x13, 0, 5):
        rows.append(super_anim_row(mission=mission, supers=((0, -1, 10),)))
    for slots in ((), (14,), (16,), (15, 17)):
        for left in (10, 5000):
            rows.append(super_anim_row(slots=slots, supers=((0, -1, left),)))
    # Only the building's weapon's Supers, each in turn.
    rows.append(super_anim_row(kind=-1, supers=((0, -1, 10),)))
    rows.append(super_anim_row(kind=2, supers=((0, -1, 10), (2, -1, 5000), (2, -1, 10))))
    rows.append(super_anim_row(kind=0, supers=((0, -1, 10), (0, -1, 5000))))
    # Health against ConditionYellow (0.5), and the names.
    for health in (51, 50, 49, 0):
        for left in (10, 5000):
            rows.append(super_anim_row(health=health, supers=((0, -1, left),)))
    # A replacement with no name (in its variant) plays nothing.
    rows.append(super_anim_row(names={(14, 0): 'SA14', (16, 0): 'SA16'},
                               supers=((0, -1, 10),)))
    rows.append(super_anim_row(names={(14, 0): 'SA14', (14, 1): 'SA14D', (15, 0): 'SA15'},
                               health=10, slots=(14,), supers=((0, -1, 10),)))
    return rows


def opening_row(*, kind=0, supers=((0, -1, 0),), health=100, strength=100,
                occupants=0, names=None, frame=9000):
    names = default_names() if names is None else names
    emu = Emu()
    install_super_anim_fixture(emu, kind=kind, cat_bits=f32_bits(999.0), mission=0x12,
                               supers=supers, slots=(), health=health, strength=strength,
                               names=names, frame=frame)
    emu.hook(STUB_OCCUPANTS, lambda _e: occupants, 0)
    uc = emu.uc
    sp = STACK_BASE + STACK_SIZE - 0x1000
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_EBP, BUILDING)
    uc.reg_write(UC_X86_REG_EDI, 0xFFFFFFFF)
    uc.reg_write(UC_X86_REG_EBX, 0)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    run_checked(uc, 0x4463F0, 0x446580, count=200_000)
    return dict(kind=kind, supers=[list(row) for row in supers], health=health,
                strength=strength, occupants=occupants, frame=frame,
                names={f'{slot}/{variant}': name for (slot, variant), name in names.items()},
                events=emu.events)


def opening_super_anim():
    rows = []
    for left in (0, 1, 14, 15, 16, 29, 30, 9000, -1, -14, -15, -16):
        rows.append(opening_row(supers=((0, -1, left),)))
    for start, left in ((9000 - 10, 20), (9000 - 10, 24), (9000 - 10, 25), (9000 - 10, 26),
                        (9000 - 10, 10), (9000, 15)):
        rows.append(opening_row(supers=((0, start, left),)))
    rows.append(opening_row(kind=-1))
    rows.append(opening_row(kind=2, supers=((0, -1, 0), (2, -1, 500), (2, -1, 3))))
    for health, occupants in ((50, 0), (51, 0), (100, 2), (10, 2), (100, -1)):
        for left in (0, 500):
            rows.append(opening_row(health=health, occupants=occupants,
                                    supers=((0, -1, left),)))
    rows.append(opening_row(names={}))
    rows.append(opening_row(names={(16, 0): 'SA16'}, health=10))
    return rows


# ---------------------------------------------------------------- AI use

RANDOM_SEED = 0x65C6D0
RANDOM_RANGED = 0x65C7E0
RNG_BYTES = 0x3F4
GAME_MODE = 0xA8B238
HOUSE_ITEMS = 0xA8022C
TECHNO_ITEMS, TECHNO_COUNT = 0xA8EC7C, 0xA8EC88
FACTORY_ITEMS, FACTORY_COUNT = 0xA83E34, 0xA83E40
INFANTRY_ITEMS, INFANTRY_COUNT = 0xA83DEC, 0xA83DF8
CELL_TABLE = 0x87F924
SCENARIO_ACTIVE = 0xA8E9A0
STORM_ACTIVE = 0xA9FAB4
MAP = 0x87F7E8
SUPERS_VTABLE = 0x7EA4E4

AI = BASE + 0x40000
ENEMY = AI
ALLY = AI + 0x6000
POINTERS = AI + 0xC000
TECHNO_POINTERS = POINTERS
FACTORY_POINTERS = POINTERS + 0x400
INFANTRY_POINTERS = POINTERS + 0x800
HOUSE_POINTERS = POINTERS + 0xC00
SUPER_POINTERS = POINTERS + 0xD00
BUILD_CONST = POINTERS + 0xE00
BUILD_TECH = POINTERS + 0xF00
OBJECTS = AI + 0x10000
OBJECT_STRIDE = 0x800
AI_TYPES = AI + 0x30000
AI_TYPE_STRIDE = 0x2000
FACTORY_BLOCKS = AI + 0x70000
OBJECT_VT = AI + 0x71000
CELL_BLOCKS = AI + 0x72000
CELL_STRIDE = 0x200
AI_SUPERS = AI + 0x80000
AI_SW_TYPES = AI + 0x81000
VALUE_LISTS = AI + 0x82000
CELL_OUT = AI + 0x83000
CELL_POINTERS = BASE + 0x200000

# The cell-offset table 0xABD490 (BSS) and its static initializer, the only
# writer (cdecl, no arguments).
CELL_OFFSETS_INIT = 0x561910

STUB_WHAT = STUBS + 0x200
STUB_LAYER = STUBS + 0x210
STUB_O_COORDS = STUBS + 0x220
STUB_OWNER = STUBS + 0x230
STUB_HIGH = STUBS + 0x240
STUB_GET_CELL = STUBS + 0x250

WHAT = {'unit': 1, 'aircraft': 2, 'building': 6, 'infantry': 0xF}
TYPE_OFFSET = {'unit': 0x6C4, 'infantry': 0x6C0, 'building': 0x520}
FACTORY_KINDS = {'BuildingType': 7, 'InfantryType': 0x10, 'UnitType': 0x28,
                 'AircraftType': 3}
# AI_FindBestRallyTarget's per-difficulty Rules vectors (the item pointers),
# read by RulesClass::ReadGeneral 0x670801..0x670AE6.
VALUE_OFFSETS = {'AIIonCannonConYardValue': 0x1198, 'AIIonCannonWarFactoryValue': 0x11B4,
                 'AIIonCannonPowerValue': 0x11D0, 'AIIonCannonTechCenterValue': 0x11EC,
                 'AIIonCannonEngineerValue': 0x1208, 'AIIonCannonThiefValue': 0x1224,
                 'AIIonCannonHarvesterValue': 0x1240, 'AIIonCannonMCVValue': 0x125C,
                 'AIIonCannonAPCValue': 0x1278, 'AIIonCannonBaseDefenseValue': 0x1294,
                 'AIIonCannonPlugValue': 0x12B0, 'AIIonCannonHelipadValue': 0x12CC,
                 'AIIonCannonTempleValue': 0x12E8}
# Distinct per difficulty so the row shows which entry was read.
VALUES = {'AIIonCannonConYardValue': (100, 90, 80), 'AIIonCannonWarFactoryValue': (70, 71, 72),
          'AIIonCannonPowerValue': (60, 61, 62), 'AIIonCannonTechCenterValue': (50, 51, 52),
          'AIIonCannonEngineerValue': (11, 12, 13), 'AIIonCannonThiefValue': (14, 15, 16),
          'AIIonCannonHarvesterValue': (17, 18, 19), 'AIIonCannonMCVValue': (20, 21, 22),
          'AIIonCannonAPCValue': (23, 24, 25), 'AIIonCannonBaseDefenseValue': (35, 36, 37),
          'AIIonCannonPlugValue': (40, 41, 42), 'AIIonCannonHelipadValue': (43, 44, 45),
          'AIIonCannonTempleValue': (46, 47, 48)}
RETAIL_VALUES = {'AIIonCannonConYardValue': (100, 100, 100),
                 'AIIonCannonWarFactoryValue': (100, 100, 100),
                 'AIIonCannonPowerValue': (60, 100, 100),
                 'AIIonCannonTechCenterValue': (100, 100, 100),
                 'AIIonCannonEngineerValue': (1, 1, 1), 'AIIonCannonThiefValue': (1, 1, 1),
                 'AIIonCannonHarvesterValue': (1, 1, 1), 'AIIonCannonMCVValue': (1, 1, 1),
                 'AIIonCannonAPCValue': (1, 1, 1), 'AIIonCannonBaseDefenseValue': (35, 35, 35),
                 'AIIonCannonPlugValue': (40, 40, 40), 'AIIonCannonHelipadValue': (20, 20, 20),
                 'AIIonCannonTempleValue': (40, 40, 40)}
# Object types by their INI keys; the Rust replay parses the same keys.
TYPE_CATALOG = [
    ('HARV', 'unit', {'Harvester': 'yes'}),
    ('MCV', 'unit', {'DeploysInto': 'CONYARD'}),
    ('APC', 'unit', {'Passengers': '5'}),
    ('TANK', 'unit', {}),
    ('DEPLOYER', 'unit', {'DeploysInto': 'PLAIN', 'Passengers': '-1'}),
    ('HARVMCV', 'unit', {'Harvester': 'yes', 'DeploysInto': 'CONYARD', 'Passengers': '3'}),
    ('CONYARD', 'building', {'Factory': 'BuildingType'}),
    ('WEAP', 'building', {'Factory': 'UnitType'}),
    ('NAVALYARD', 'building', {'Factory': 'UnitType', 'Naval': 'yes', 'Power': '-20'}),
    ('BARRACKS', 'building', {'Factory': 'InfantryType', 'Power': '-10'}),
    ('POWER', 'building', {'Power': '100'}),
    ('DRAIN', 'building', {'Power': '-50'}),
    ('DEFENSE', 'building', {'IsBaseDefense': 'yes', 'Power': '-5'}),
    ('POWERDEFENSE', 'building', {'IsBaseDefense': 'yes', 'Power': '10'}),
    ('PLUG', 'building', {'IsPlug': 'yes'}),
    ('TEMPLE', 'building', {'IsTemple': 'yes', 'IsPlug': 'yes'}),
    ('PAD', 'building', {'HoverPad': 'yes'}),
    ('TECH', 'building', {'Power': '-100'}),
    ('PLAIN', 'building', {}),
    ('ENGI', 'infantry', {'Engineer': 'yes'}),
    ('THIEF', 'infantry', {'VehicleThief': 'yes'}),
    ('ENGITHIEF', 'infantry', {'Engineer': 'yes', 'VehicleThief': 'yes'}),
    ('GI', 'infantry', {}),
    ('JET', 'aircraft', {}),
]
BUILD_CONST_TYPES = ['CONYARD']
BUILD_TECH_TYPES = ['TECH', 'POWER', 'PAD']
# MapClass +0xF4/+0xFC/+0x100/+0x104/+0x108: the playfield the native
# IsCellInPlayfield 0x578460 tests (every cell lookup misses, so the level
# and slope are the dummy's zeros).
PLAYFIELD = {'base': 40, 'off_fc': 2, 'off_100': 4, 'off_104': 36, 'off_108': 36}
LAYER_GROUND, LAYER_AIR = 2, 3


def seed_scenario_rng(emu, seed):
    emu.invoke(RANDOM_SEED, ecx=SCENARIO_RANDOM, args=[seed])


def record_draws(emu):
    """RandomRanged 0x65C7E0 runs natively; each call's stream and bounds are
    recorded."""
    def entered(uc, _address, _size, _data):
        sp = uc.reg_read(UC_X86_REG_ESP)
        emu.events.append(['draw', uc.reg_read(UC_X86_REG_ECX) - SCENARIO_RANDOM + 0x218,
                           i32(emu.read32(sp + 4)), i32(emu.read32(sp + 8))])
    emu.uc.hook_add(UC_HOOK_CODE, entered, begin=RANDOM_RANGED, end=RANDOM_RANGED)


def rng_state(emu):
    return bytes(emu.uc.mem_read(SCENARIO_RANDOM, RNG_BYTES)).hex()


def install_playfield(emu):
    emu.write32(CELL_TABLE, CELL_POINTERS)
    for name, offset in (('base', 0xF4), ('off_fc', 0xFC), ('off_100', 0x100),
                         ('off_104', 0x104), ('off_108', 0x108)):
        emu.write32(MAP + offset, PLAYFIELD[name])


def install_houses(emu, *, difficulty=0, enemy_index=1, allies=0b100):
    """HouseClass::Array: the computer house, its enemy, and a house it
    counts as an ally (`+0x5788` bit 2)."""
    for index, house in enumerate((HOUSE, ENEMY, ALLY)):
        emu.write32(HOUSE_POINTERS + 4 * index, house)
        emu.write32(house + 0x30, index)
    emu.write32(HOUSE_ITEMS, HOUSE_POINTERS)
    emu.write32(HOUSE + 0x184, difficulty)
    emu.write32(HOUSE + 0x5600, enemy_index)
    emu.write32(HOUSE + 0x5788, allies)
    # The constructor's preferred target (type 1, no cell; 0x4F5A77/0x4F5A81).
    emu.write32(HOUSE + 0x54EC, 1)


def install_types(emu):
    types = {}
    for index, (name, what, _keys) in enumerate(TYPE_CATALOG):
        types[name] = AI_TYPES + index * AI_TYPE_STRIDE
    for name, what, keys in TYPE_CATALOG:
        ty = types[name]
        for key, value in keys.items():
            if key == 'Harvester':
                write8(emu, ty + 0xE0E, 1)
            elif key == 'DeploysInto':
                emu.write32(ty + 0x404, types[value])
            elif key == 'Passengers':
                emu.write32(ty + 0x5E0, int(value))
            elif key == 'Factory':
                emu.write32(ty + 0xEB8, FACTORY_KINDS[value])
            elif key == 'Naval':
                write8(emu, ty + 0xCCE, 1)
            elif key == 'Power':
                power = int(value)
                emu.write32(ty + 0xEE0, max(power, 0))
                emu.write32(ty + 0xEE4, max(-power, 0))
            elif key == 'IsBaseDefense':
                write8(emu, ty + 0x1706, 1)
            elif key == 'IsPlug':
                write8(emu, ty + 0x154D, 1)
            elif key == 'IsTemple':
                write8(emu, ty + 0x154C, 1)
            elif key == 'HoverPad':
                write8(emu, ty + 0x154E, 1)
            elif key == 'Engineer':
                write8(emu, ty + 0xEC3, 1)
            elif key == 'VehicleThief':
                write8(emu, ty + 0xEC6, 1)
            else:
                raise OracleError(f'unknown catalog key {key}')
    for base, names, items in ((0x8B0, BUILD_CONST_TYPES, BUILD_CONST),
                               (0x920, BUILD_TECH_TYPES, BUILD_TECH)):
        for slot, name in enumerate(names):
            emu.write32(items + 4 * slot, types[name])
        emu.write32(RULES + base, items)
        emu.write32(RULES + base + 0xC, len(names))
    return types


def install_values(emu, values):
    for slot, (key, offset) in enumerate(sorted(VALUE_OFFSETS.items())):
        items = VALUE_LISTS + slot * 0x10
        for index, value in enumerate(values[key]):
            emu.write32(items + 4 * index, value)
        emu.write32(RULES + offset, items)


SCENARIO_RANDOM = FAKE + 0x22000 + 0x218


def techno(owner='enemy', type='TANK', *, layer=LAYER_GROUND, alive=True, limbo=False,
           coords=(30 * 256 + 128, 30 * 256 + 128, 0), cloak=0, stage=0, factory=None):
    return dict(owner=owner, type=type, layer=layer, alive=alive, limbo=limbo,
                coords=list(coords), cloak=cloak, stage=stage, factory=factory)


def at(x, y, z=0):
    return (x * 256 + 128, y * 256 + 128, z)


def best_rally_row(objects, *, difficulty=1, values=VALUES, seed=31):
    emu = Emu()
    install_houses(emu, difficulty=difficulty)
    install_playfield(emu)
    install_values(emu, values)
    types = install_types(emu)
    whats = {name: what for name, what, _keys in TYPE_CATALOG}
    owners = {'self': HOUSE, 'enemy': ENEMY, 'ally': ALLY}
    facts = {}
    factories = []
    emu.write32(OBJECT_VT + 0x2C, STUB_WHAT)
    emu.write32(OBJECT_VT + 0x48, STUB_O_COORDS)
    emu.write32(OBJECT_VT + 0x78, STUB_LAYER)
    for index, obj in enumerate(objects):
        this = OBJECTS + index * OBJECT_STRIDE
        what = whats[obj['type']]
        facts[this] = (WHAT[what], obj)
        emu.write32(TECHNO_POINTERS + 4 * index, this)
        emu.write32(this, OBJECT_VT)
        emu.write32(this + 0x21C, owners[obj['owner']])
        emu.write32(this + 0x220, obj['cloak'])
        write8(emu, this + 0x90, obj['alive'])
        write8(emu, this + 0x81, obj['limbo'])
        if what in TYPE_OFFSET:
            emu.write32(this + TYPE_OFFSET[what], types[obj['type']])
        if what == 'building':
            write8(emu, this + 0x6ED, obj['stage'])
        if obj['factory'] is not None:
            factory = FACTORY_BLOCKS + len(factories) * 0x100
            emu.write32(factory + 0x58, this)
            emu.write32(factory + 0x38, obj['factory']['rate'])
            write8(emu, factory + 0x70, obj['factory']['suspended'])
            factories.append(factory)
    # A factory building something else stands first.
    idle = FACTORY_BLOCKS + 0xF00
    emu.write32(idle + 0x58, OBJECTS + 0x1F * OBJECT_STRIDE)
    emu.write32(idle + 0x38, 9)
    factories.insert(0, idle)
    for slot, factory in enumerate(factories):
        emu.write32(FACTORY_POINTERS + 4 * slot, factory)
    emu.write32(TECHNO_ITEMS, TECHNO_POINTERS)
    emu.write32(TECHNO_COUNT, len(objects))
    emu.write32(FACTORY_ITEMS, FACTORY_POINTERS)
    emu.write32(FACTORY_COUNT, len(factories))

    def what_am_i(e):
        return facts[e.uc.reg_read(UC_X86_REG_ECX)][0]

    def layer(e):
        e.events.append(['layer', (e.uc.reg_read(UC_X86_REG_ECX) - OBJECTS) // OBJECT_STRIDE])
        return facts[e.uc.reg_read(UC_X86_REG_ECX)][1]['layer']

    def coords(e):
        out = e.arg(0)
        write_coord(e, out, facts[e.uc.reg_read(UC_X86_REG_ECX)][1]['coords'])
        return out

    emu.hook(STUB_WHAT, what_am_i, 0)
    emu.hook(STUB_LAYER, layer, 0)
    emu.hook(STUB_O_COORDS, coords, 4)
    seed_scenario_rng(emu, seed)
    record_draws(emu)
    emu.uc.mem_write(CELL_OUT, struct.pack('<hh', -7, -7))
    emu.invoke(0x50CBF0, ecx=HOUSE, args=[CELL_OUT])
    return dict(difficulty=difficulty, values={key: list(v) for key, v in values.items()},
                seed=seed, objects=objects, events=emu.events, cell=read_cell(emu, CELL_OUT),
                rng_after=rng_state(emu))


def best_rally_target():
    rows = []
    # Each kind alone, at each difficulty: its value decides nothing, the
    # one candidate is the target (RandomRanged(0, 0)).
    for name, what, _keys in TYPE_CATALOG:
        for difficulty in (0, 1, 2):
            rows.append(best_rally_row([techno(type=name, coords=at(31, 32))],
                                       difficulty=difficulty))
    # Two kinds: the larger value wins, in either array order.
    pairs = [('HARV', 'MCV'), ('APC', 'TANK'), ('CONYARD', 'WEAP'), ('WEAP', 'NAVALYARD'),
             ('NAVALYARD', 'BARRACKS'), ('POWER', 'DRAIN'), ('DEFENSE', 'POWERDEFENSE'),
             ('PLUG', 'TEMPLE'), ('PAD', 'TECH'), ('TECH', 'PLAIN'), ('ENGI', 'THIEF'),
             ('ENGITHIEF', 'GI'), ('JET', 'GI'), ('DEPLOYER', 'HARVMCV'), ('TANK', 'JET')]
    for first, second in pairs:
        for order in ((first, second), (second, first)):
            rows.append(best_rally_row([techno(type=order[0], coords=at(31, 32)),
                                        techno(type=order[1], coords=at(40, 25))]))
    # Ties: the final RandomRanged(0, n - 1) picks, with retail values.
    for seed in (31, 32, 33, 34, 7, 1234567):
        rows.append(best_rally_row([techno(type='CONYARD', coords=at(31, 32)),
                                    techno(type='WEAP', coords=at(40, 25)),
                                    techno(type='TECH', coords=at(35, 35)),
                                    techno(type='POWER', coords=at(36, 36))],
                                   values=RETAIL_VALUES, seed=seed, difficulty=1))
    # Who is a candidate: other houses' objects, limbo, dead, airborne.
    rows.append(best_rally_row([techno(owner='self', type='CONYARD', coords=at(31, 32)),
                                techno(owner='ally', type='CONYARD', coords=at(33, 32)),
                                techno(type='GI', coords=at(40, 25))]))
    for flags in (dict(limbo=True), dict(alive=False), dict(layer=LAYER_AIR), dict(layer=4),
                  dict(layer=0)):
        rows.append(best_rally_row([techno(type='CONYARD', coords=at(31, 32), **flags),
                                    techno(type='GI', coords=at(40, 25))]))
        rows.append(best_rally_row([techno(type='CONYARD', coords=at(31, 32), **flags)]))
    # Off the playfield the value is zero, but the object stays a candidate.
    rows.append(best_rally_row([techno(type='CONYARD', coords=at(10, 10))]))
    rows.append(best_rally_row([techno(type='CONYARD', coords=at(10, 10)),
                                techno(type='GI', coords=at(40, 25))]))
    rows.append(best_rally_row([techno(type='GI', coords=at(70, 60)),
                                techno(type='TANK', coords=at(10, 10))]))
    # Coordinates to cells toward zero, at the playfield's edges.
    for coords in ((-100, 300, 0), (-300, -300, 0), (24 * 256 + 255, 24 * 256 + 255, 0),
                   (25 * 256, 24 * 256, 0), (61 * 256 + 128, 61 * 256 + 128, 0),
                   (62 * 256 + 128, 61 * 256 + 128, 0), (60 * 256, 25 * 256, 0),
                   (31 * 256 + 128, 32 * 256 + 128, 2000)):
        rows.append(best_rally_row([techno(type='TANK', coords=coords),
                                    techno(type='HARV', coords=at(40, 41))]))
    # Cloaked objects of any house draw RandomRanged(0, best + 10).
    for seed in (31, 99):
        rows.append(best_rally_row([techno(type='GI', coords=at(40, 25)),
                                    techno(owner='self', type='TANK', coords=at(31, 32),
                                           cloak=2),
                                    techno(type='TANK', coords=at(33, 32), cloak=2),
                                    techno(type='POWER', coords=at(34, 34), cloak=1),
                                    techno(type='POWER', coords=at(35, 34), cloak=3),
                                    techno(type='CONYARD', coords=at(36, 32), cloak=2,
                                           limbo=True)],
                                   seed=seed))
    # A building's cloak stage 15 draws as well (BuildingClass +0x6ED).
    for stage in (14, 15):
        rows.append(best_rally_row([techno(type='WEAP', coords=at(33, 32), stage=stage),
                                    techno(type='GI', coords=at(40, 25))]))
    # A Hard house also takes what an enemy factory is building.
    for difficulty in (0, 1):
        for factory in (dict(rate=5, suspended=False), dict(rate=0, suspended=False),
                        dict(rate=5, suspended=True)):
            rows.append(best_rally_row([techno(type='CONYARD', limbo=True, coords=(0, 0, 0),
                                               factory=factory)],
                                       difficulty=difficulty))
            rows.append(best_rally_row([techno(type='WEAP', limbo=True,
                                               coords=at(33, 32), factory=factory),
                                        techno(type='GI', coords=at(40, 25))],
                                       difficulty=difficulty))
    rows.append(best_rally_row([techno(owner='self', type='WEAP', limbo=True,
                                       coords=at(33, 32), factory=dict(rate=5,
                                                                       suspended=False))],
                               difficulty=0))
    # No enemy object: no target.
    rows.append(best_rally_row([]))
    rows.append(best_rally_row([techno(owner='self', type='WEAP', coords=at(33, 32))]))
    # A larger mix, several seeds.
    mix = [techno(type='GI', coords=at(40, 25)), techno(type='HARV', coords=at(41, 26)),
           techno(owner='ally', type='TANK', coords=at(45, 30), cloak=2),
           techno(type='POWER', coords=at(30, 31)), techno(type='POWER', coords=at(31, 31)),
           techno(type='DEFENSE', coords=at(32, 30), cloak=2),
           techno(type='JET', coords=at(36, 36), layer=LAYER_AIR),
           techno(type='POWER', coords=at(29, 31)), techno(type='ENGI', coords=at(28, 30))]
    for seed in (31, 5, 77, 2024):
        for difficulty in (0, 1, 2):
            rows.append(best_rally_row(mix, values=RETAIL_VALUES, seed=seed,
                                       difficulty=difficulty))
    return rows


def install_supers(emu, kinds):
    """The house's Supers vector (`+0x254`, DynamicVectorClass 0x7EA4E4):
    Super i of `kinds[i] = (Type=, charged)`."""
    emu.write32(HOUSE + 0x254, SUPERS_VTABLE)
    emu.write32(HOUSE + 0x258, SUPER_POINTERS)
    emu.write32(HOUSE + 0x25C, len(kinds))
    emu.write32(HOUSE + 0x264, len(kinds))
    for index, (kind, charged) in enumerate(kinds):
        this = AI_SUPERS + index * 0x100
        ty = AI_SW_TYPES + index * 0x100
        emu.write32(SUPER_POINTERS + 4 * index, this)
        emu.write32(this + 0x28, ty)
        write8(emu, this + 0x6F, charged)
        emu.write32(ty + 0xB4, kind)


def super_index(this):
    return (this - AI_SUPERS) // 0x100


def record_fire(emu):
    """Fire_SW 0x4FAE50 (thiscall, RET 8) is a recorded stub."""
    def fire(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != HOUSE:
            raise OracleError('Fire_SW called on another house')
        e.events.append(['fire', i32(e.arg(0)), read_cell(e, e.arg(1))])
        return 1
    emu.hook(0x4FAE50, fire, 8)


def try_fire_row(kinds, *, game_mode=1, human=False, control=False, enemy_index=1,
                 storm=False, rally=(30, 31), defense=(0, 0), defense_frame=-100,
                 defense_frames=50, frame=1000):
    emu = Emu()
    install_houses(emu, enemy_index=enemy_index)
    install_supers(emu, kinds)
    emu.write32(GAME_MODE, game_mode)
    write8(emu, HOUSE + 0x1EC, human)
    write8(emu, HOUSE + 0x1ED, control)
    write8(emu, STORM_ACTIVE, storm)
    emu.uc.mem_write(HOUSE + 0x54F4, struct.pack('<hh', *defense))
    emu.write32(HOUSE + 0x54FC, defense_frame)
    emu.write32(RULES + 0xEE0, defense_frames)
    emu.write32(FRAME, frame)
    record_fire(emu)

    def picker(name, pops):
        def answer(e):
            e.events.append([name, super_index(e.arg(0))])
        emu.hook({'ground': 0x509CD0, 'psychic_dominator': 0x50A150,
                  'genetic_mutator': 0x509F60}[name], answer, pops)

    def best(e):
        e.events.append(['best_rally_target'])
        e.uc.mem_write(e.arg(0), struct.pack('<hh', *rally))
        return e.arg(0)

    picker('ground', 4)
    picker('psychic_dominator', 4)
    picker('genetic_mutator', 4)
    emu.hook(0x50CBF0, best, 4)
    emu.invoke(0x5098F0, ecx=HOUSE)
    return dict(kinds=[[kind, charged] for kind, charged in kinds], game_mode=game_mode,
                human=human, control=control, enemy_index=enemy_index, storm=storm,
                rally=list(rally), defense=list(defense), defense_frame=defense_frame,
                defense_frames=defense_frames, frame=frame, events=emu.events)


def try_fire():
    rows = []
    every = [(kind, True) for kind in range(12)]
    rows.append(try_fire_row(every))
    rows.append(try_fire_row([(kind, False) for kind in range(12)]))
    rows.append(try_fire_row(list(reversed(every)), defense=(20, 21), defense_frame=990))
    for game_mode, human, control in ((1, True, False), (1, False, True), (0, False, True),
                                      (0, True, False), (0, False, False)):
        rows.append(try_fire_row(every, game_mode=game_mode, human=human, control=control))
    rows.append(try_fire_row(every, enemy_index=-1))
    rows.append(try_fire_row(every, storm=True))
    rows.append(try_fire_row(every, rally=(0, 0)))
    rows.append(try_fire_row(every, rally=(0, 5)))
    rows.append(try_fire_row([(0, True), (0, True), (2, True)], rally=(5, 0)))
    # Force Shield: the alert's cell while the alert is younger than
    # AISuperDefenseFrames (`0x00509A7F..0x00509A99`).
    for defense, defense_frame, defense_frames, frame in (
            ((20, 21), 990, 50, 1000), ((20, 21), 950, 50, 1000), ((20, 21), 951, 50, 1000),
            ((20, 21), 949, 50, 1000), ((20, 21), -100, 50, 0), ((20, 21), -100, 50, -51),
            ((20, 21), 1000, 0, 1000), ((20, 21), 1000, -1, 999), ((0, 0), 990, 50, 1000),
            ((0, 7), 990, 50, 1000), ((7, 0), 990, 50, 1000),
            ((20, 21), 0x7FFFFFF0, 0x20, 0x7FFFFFF0), ((20, 21), -100, 50, -100)):
        rows.append(try_fire_row([(10, True)], defense=defense, defense_frame=defense_frame,
                                 defense_frames=defense_frames, frame=frame))
    return rows


def ground_rally_row(*, enemy_index=1, enemy_base=(30, 31), enemy_alternate=(0, 0),
                     own_base=(10, 11), own_alternate=(0, 0), found=(32, 33), supers=3,
                     fired=1, frame=1000):
    emu = Emu()
    install_houses(emu, enemy_index=enemy_index)
    install_supers(emu, [(5, True)] * supers)
    emu.write32(FRAME, frame)
    for house, base, alternate in ((HOUSE, own_base, own_alternate),
                                   (ENEMY, enemy_base, enemy_alternate)):
        emu.uc.mem_write(house + 0x5490, struct.pack('<hh', *base))
        emu.uc.mem_write(house + 0x5494, struct.pack('<hh', *alternate))
    record_fire(emu)

    def nearby(e):
        args = [i32(e.arg(index)) for index in range(15)]
        e.events.append(['find_nearby', read_cell(e, e.arg(1)), args[2:12],
                         read_cell(e, e.arg(12)), args[13:]])
        e.uc.mem_write(e.arg(0), struct.pack('<hh', *found))
        return e.arg(0)

    emu.hook(0x56DC20, nearby, 0x3C)
    emu.invoke(0x509CD0, ecx=HOUSE, args=[AI_SUPERS + fired * 0x100])
    return dict(enemy_index=enemy_index, enemy_base=list(enemy_base),
                enemy_alternate=list(enemy_alternate), own_base=list(own_base),
                own_alternate=list(own_alternate), found=list(found), supers=supers,
                fired=fired, events=emu.events)


def ground_rally_point():
    return [ground_rally_row(), ground_rally_row(enemy_alternate=(40, 41)),
            ground_rally_row(enemy_base=(0, 0)), ground_rally_row(enemy_base=(0, 0),
                                                                 enemy_alternate=(0, 0)),
            ground_rally_row(enemy_index=-1), ground_rally_row(enemy_index=-1,
                                                               own_alternate=(12, 13)),
            ground_rally_row(found=(0, 0)), ground_rally_row(found=(-2, -2)),
            ground_rally_row(found=(32766, 5)), ground_rally_row(supers=1, fired=0),
            ground_rally_row(supers=5, fired=4)]


def gen_mutator_row(objects):
    """`objects`: (kind, owner, cell, extra) in spawn order; each infantry is
    InfantryClass::Array's next entry, each lists itself in its cell like
    Unlimbo: a non-building at the head, a building at the tail."""
    emu = Emu()
    install_houses(emu)
    install_playfield(emu)
    install_supers(emu, [(9, True), (9, True)])
    write8(emu, SCENARIO_ACTIVE, 1)
    record_fire(emu)
    owners = {'self': HOUSE, 'enemy': ENEMY, 'ally': ALLY}
    cells = {}
    facts = {}
    infantry = []

    def cell_block(cell):
        cell = tuple(cell)
        if cell not in cells:
            block = CELL_BLOCKS + len(cells) * CELL_STRIDE
            emu.uc.mem_write(block + 0x24, struct.pack('<hh', *cell))
            cells[cell] = block
        return cells[cell]

    emu.write32(OBJECT_VT + 0x2C, STUB_WHAT)
    emu.write32(OBJECT_VT + 0x3C, STUB_OWNER)
    emu.write32(OBJECT_VT + 0x54, STUB_HIGH)
    emu.write32(OBJECT_VT + 0x1BC, STUB_GET_CELL)
    for index, (kind, owner, cell, extra) in enumerate(objects):
        this = OBJECTS + index * OBJECT_STRIDE
        bridge = extra.get('bridge', False)
        facts[this] = dict(what=WHAT.get(kind, 0x24), owner=owners[owner],
                           high=extra.get('high', False), cell=tuple(cell))
        emu.write32(this, OBJECT_VT)
        write8(emu, this + 0x8C, bridge)
        write8(emu, this + 0x81, extra.get('limbo', False))
        block = cell_block(cell)
        head = block + (0xE8 if bridge else 0xE4)
        if not extra.get('limbo', False):
            if kind == 'building':
                tail = head - 0x30
                while emu.read32(tail + 0x30):
                    tail = emu.read32(tail + 0x30)
                emu.write32(tail + 0x30, this)
            else:
                emu.write32(this + 0x30, emu.read32(head))
                emu.write32(head, this)
        if kind == 'infantry':
            emu.write32(INFANTRY_POINTERS + 4 * len(infantry), this)
            infantry.append(this)
    emu.write32(INFANTRY_ITEMS, INFANTRY_POINTERS)
    emu.write32(INFANTRY_COUNT, len(infantry))

    def lookup(e):
        return cell_block(read_cell(e, e.arg(0)))

    emu.hook(0x5657A0, lookup, 4)
    emu.hook(STUB_WHAT, lambda e: facts[e.uc.reg_read(UC_X86_REG_ECX)]['what'], 0)
    emu.hook(STUB_OWNER, lambda e: facts[e.uc.reg_read(UC_X86_REG_ECX)]['owner'], 0)
    emu.hook(STUB_HIGH, lambda e: int(facts[e.uc.reg_read(UC_X86_REG_ECX)]['high']), 0)
    emu.hook(STUB_GET_CELL,
             lambda e: cell_block(facts[e.uc.reg_read(UC_X86_REG_ECX)]['cell']), 0)
    emu.invoke(CELL_OFFSETS_INIT)
    emu.invoke(0x509F60, ecx=HOUSE, args=[AI_SUPERS + 0x100])
    return dict(objects=[[kind, owner, list(cell), extra]
                         for kind, owner, cell, extra in objects],
                events=emu.events)


def gen_mutator():
    def inf(owner, cell, **extra):
        return ('infantry', owner, cell, extra)

    def unit(owner, cell, **extra):
        return ('unit', owner, cell, extra)

    rows = [
        gen_mutator_row([inf('enemy', (30, 31))]),
        gen_mutator_row([inf('self', (30, 31))]),
        gen_mutator_row([inf('ally', (30, 31))]),
        gen_mutator_row([inf('self', (30, 31)), inf('enemy', (31, 31))]),
        # The densest neighbourhood; ties keep the later infantry (the scan
        # runs last to first and needs a strictly larger count).
        gen_mutator_row([inf('enemy', (30, 31)), inf('enemy', (30, 31)),
                         inf('enemy', (40, 41)), inf('enemy', (41, 41)),
                         inf('enemy', (40, 42))]),
        gen_mutator_row([inf('enemy', (30, 31)), inf('enemy', (31, 31)),
                         inf('enemy', (40, 41)), inf('enemy', (41, 41))]),
        # The spread cells (table entries 0..=9): the later infantry wins a
        # tie, so (30, 30) wins only when it sees the other one.
        gen_mutator_row([inf('enemy', (30, 30)), inf('enemy', (29, 28))]),
        gen_mutator_row([inf('enemy', (30, 30)), inf('enemy', (30, 28))]),
        gen_mutator_row([inf('enemy', (30, 30)), inf('enemy', (31, 28))]),
        gen_mutator_row([inf('enemy', (30, 30)), inf('enemy', (31, 29))]),
        gen_mutator_row([inf('enemy', (30, 30)), inf('enemy', (29, 31))]),
        gen_mutator_row([inf('enemy', (30, 30)), inf('enemy', (32, 30))]),
        # A cell's list after GetInfantry stops at its first non-infantry.
        gen_mutator_row([inf('enemy', (30, 31)), unit('enemy', (30, 31)),
                         inf('enemy', (30, 31)), inf('self', (36, 36))]),
        gen_mutator_row([unit('enemy', (30, 31)), inf('enemy', (30, 31)),
                         inf('enemy', (30, 31)), inf('self', (36, 36))]),
        gen_mutator_row([inf('enemy', (30, 31)), ('building', 'enemy', (30, 31), {}),
                         inf('enemy', (30, 31)), inf('self', (36, 36))]),
        # High-flying infantry do not count; neither does an own or allied one.
        gen_mutator_row([inf('enemy', (30, 31), high=True), inf('self', (31, 31))]),
        gen_mutator_row([inf('enemy', (30, 31), high=True), inf('enemy', (30, 31)),
                         inf('ally', (31, 31))]),
        # The bridge list is read for an infantry on a bridge.
        gen_mutator_row([inf('enemy', (30, 31), bridge=True), inf('self', (31, 31))]),
        gen_mutator_row([inf('enemy', (30, 31), bridge=True),
                         inf('self', (31, 31), bridge=True)]),
        # A limbo infantry is no centre and lists nowhere.
        gen_mutator_row([inf('enemy', (30, 31), limbo=True), inf('self', (36, 36))]),
        # Off the playfield nothing fires.
        gen_mutator_row([inf('enemy', (10, 10))]),
        gen_mutator_row([inf('enemy', (10, 10)), inf('enemy', (11, 10)),
                         inf('enemy', (40, 41))]),
        gen_mutator_row([]),
    ]
    return rows


# ---------------------------------------------------------------- ai_psydom

FOOT_ITEMS, FOOT_COUNT = 0x8B3DC4, 0x8B3DD0
FOOT_POINTERS = POINTERS + 0x1000
PSY_TYPES = AI + 0x84000
PSY_TYPE_STRIDE = 0x1000
STUB_PSY_TYPE = STUBS + 0x260
STUB_CURTAINED = STUBS + 0x270
# AbstractClass+0x14: Techno (bit 0, 0x6F322F), Object (bit 1) and Foot
# (bit 2, FootClass's constructor at 0x4D34DD).
ABSTRACT_FLAGS = {'unit': 7, 'infantry': 7, 'aircraft': 7, 'building': 3}


def psydom_ai_row(objects, *, psydom=0, enemy_index=1):
    """HouseClass::AI_Fire_PsyDom 0x50A150 for the computer house's charged
    Dominator (Super 0). `objects`: (kind, owner, cell, extra) in spawn
    order; each Foot is FootClass::Array's next entry and lists itself in
    its cell like Unlimbo: a non-building at the head of its ground list
    (the bridge list for extra bridge), a building at the tail. Owner 'none'
    is an object of no house. extra: limbo, high (vt+0x54), immune
    (ImmuneToPsionics, type +0xD35), balloon (BalloonHover, +0xD6A), curtain
    (vt+0x160). CanBePermaMindControlled 0x53C450 and Is_Cell_In_Playfield
    0x578460 run natively."""
    emu = Emu()
    install_houses(emu, enemy_index=enemy_index)
    install_playfield(emu)
    install_supers(emu, [(7, True)])
    write8(emu, SCENARIO_ACTIVE, 1)
    emu.write32(G_PSYDOM_STATUS, psydom)
    record_fire(emu)
    owners = {'self': HOUSE, 'enemy': ENEMY, 'ally': ALLY, 'none': 0}
    cells = {}
    facts = {}
    feet = []

    def cell_block(cell):
        cell = tuple(cell)
        if cell not in cells:
            block = CELL_BLOCKS + len(cells) * CELL_STRIDE
            emu.uc.mem_write(block + 0x24, struct.pack('<hh', *cell))
            cells[cell] = block
        return cells[cell]

    for offset, stub in ((0x2C, STUB_WHAT), (0x3C, STUB_OWNER), (0x54, STUB_HIGH),
                         (0x84, STUB_PSY_TYPE), (0x160, STUB_CURTAINED),
                         (0x1BC, STUB_GET_CELL)):
        emu.write32(OBJECT_VT + offset, stub)
    for index, (kind, owner, cell, extra) in enumerate(objects):
        this = OBJECTS + index * OBJECT_STRIDE
        ty = PSY_TYPES + index * PSY_TYPE_STRIDE
        facts[this] = dict(what=WHAT[kind], owner=owners[owner], high=extra.get('high', False),
                           cell=tuple(cell), type=ty, curtain=extra.get('curtain', False))
        emu.write32(this, OBJECT_VT)
        write8(emu, this + 0x14, ABSTRACT_FLAGS[kind])
        write8(emu, this + 0x81, extra.get('limbo', False))
        write8(emu, ty + 0xD35, extra.get('immune', False))
        write8(emu, ty + 0xD6A, extra.get('balloon', False))
        block = cell_block(cell)
        head = block + (0xE8 if extra.get('bridge', False) else 0xE4)
        if not extra.get('limbo', False):
            if kind == 'building':
                tail = head - 0x30
                while emu.read32(tail + 0x30):
                    tail = emu.read32(tail + 0x30)
                emu.write32(tail + 0x30, this)
            else:
                emu.write32(this + 0x30, emu.read32(head))
                emu.write32(head, this)
        if kind != 'building':
            emu.write32(FOOT_POINTERS + 4 * len(feet), this)
            feet.append(this)
    emu.write32(FOOT_ITEMS, FOOT_POINTERS)
    emu.write32(FOOT_COUNT, len(feet))

    def fact(e, key):
        return facts[e.uc.reg_read(UC_X86_REG_ECX)][key]

    emu.hook(0x5657A0, lambda e: cell_block(read_cell(e, e.arg(0))), 4)
    emu.hook(STUB_WHAT, lambda e: fact(e, 'what'), 0)
    emu.hook(STUB_OWNER, lambda e: fact(e, 'owner'), 0)
    emu.hook(STUB_HIGH, lambda e: int(fact(e, 'high')), 0)
    emu.hook(STUB_PSY_TYPE, lambda e: fact(e, 'type'), 0)
    emu.hook(STUB_CURTAINED, lambda e: int(fact(e, 'curtain')), 0)
    emu.hook(STUB_GET_CELL, lambda e: cell_block(fact(e, 'cell')), 0)
    emu.invoke(CELL_OFFSETS_INIT)
    emu.invoke(0x50A150, ecx=HOUSE, args=[AI_SUPERS])
    return dict(psydom=psydom, enemy_index=enemy_index,
                objects=[[kind, owner, list(cell), extra]
                         for kind, owner, cell, extra in objects],
                events=emu.events)


def psydom_ai():
    def foot(kind, owner, cell, **extra):
        return (kind, owner, cell, extra)

    def inf(owner, cell, **extra):
        return foot('infantry', owner, cell, **extra)

    def tank(owner, cell, **extra):
        return foot('unit', owner, cell, **extra)

    row = psydom_ai_row
    return [
        row([inf('enemy', (30, 31))]),
        # Gates: a Dominator running (any status but 0), no enemy.
        row([inf('enemy', (30, 31))], psydom=5),
        row([inf('enemy', (30, 31))], enemy_index=-1),
        # Own and allied objects never count; one of no house does.
        row([inf('self', (30, 31))]),
        row([inf('ally', (30, 31))]),
        row([tank('none', (30, 31))]),
        # The house's own unit is a centre like any other.
        row([tank('self', (30, 31)), inf('enemy', (32, 31))]),
        # CanBePermaMindControlled's refusals and the air.
        row([inf('enemy', (30, 31), immune=True), inf('self', (36, 36))]),
        row([tank('enemy', (30, 31), balloon=True), inf('self', (36, 36))]),
        row([tank('enemy', (30, 31), curtain=True), inf('self', (36, 36))]),
        row([foot('aircraft', 'enemy', (30, 31), high=True), inf('self', (36, 36))]),
        row([foot('aircraft', 'enemy', (30, 31)), inf('self', (36, 36))]),
        # The densest neighbourhood; a tie keeps the later Foot.
        row([inf('enemy', (30, 31)), inf('enemy', (30, 31)), tank('enemy', (44, 40)),
             tank('enemy', (45, 40)), tank('enemy', (44, 41))]),
        row([tank('enemy', (30, 31)), tank('enemy', (31, 31)), tank('enemy', (44, 40)),
             tank('enemy', (45, 40))]),
        # The sweep: table entries 0..=37 (the radius-3 band's 37 inclusive).
        row([tank('enemy', (30, 30)), tank('enemy', (33, 30)), tank('enemy', (40, 40))]),
        row([tank('enemy', (30, 30)), tank('enemy', (34, 30)), tank('enemy', (40, 40))]),
        row([tank('enemy', (30, 30)), tank('enemy', (32, 32)), tank('enemy', (40, 40))]),
        row([tank('enemy', (30, 30)), tank('enemy', (33, 33)), tank('enemy', (40, 40))]),
        # The last entry, 37 at (-1,-4), counts and entry 38 at (0,-4) does not;
        # the partner never sees the centre ((1,4) and (0,4) lie past 37).
        row([tank('enemy', (30, 34)), tank('enemy', (29, 30))]),
        row([tank('enemy', (30, 34)), tank('enemy', (30, 30))]),
        # A ground list counts from its head while each object is a Foot.
        row([tank('enemy', (30, 31)), ('building', 'enemy', (30, 31), {}),
             tank('enemy', (30, 31)), inf('self', (36, 36))]),
        # The bridge list is never read, though its Foot is a centre.
        row([tank('enemy', (30, 31), bridge=True), tank('enemy', (31, 31))]),
        row([tank('enemy', (30, 31), bridge=True), inf('self', (36, 36))]),
        # A limbo Foot is no centre and lists nowhere.
        row([tank('enemy', (30, 31), limbo=True), inf('self', (36, 36))]),
        # Off the playfield nothing fires.
        row([inf('enemy', (10, 10))]),
        row([]),
    ]

# ---------------------------------------------------------------- chrono_process

# A Chrono Warp's Teleport and its owner as Launch case 4 leaves them
# (0x6CC9F2..0x6CCB43): a fresh TeleportLocomotionClass (constructor
# 0x718000, Link_To_Object 0x55A710, Begin_Piggyback 0x719E90, all native)
# over the owner's locomotor; the owner latched (+0x27C) with its
# destination (+0x288).
CHRONO = BASE + 0x300000
TELEPORT = CHRONO
OWNER = CHRONO + 0x1000
OWNER_VT = CHRONO + 0x2000
OWNER_TYPE = CHRONO + 0x3000
STASH = CHRONO + 0x4000
STASH_VT = CHRONO + 0x5000
WARP_ANIM_TYPE = CHRONO + 0x6000
CHRONO_OUT_SOUND, CHRONO_IN_SOUND = 12, 13
SOURCE_COORD = (21 * 256 + 128, 21 * 256 + 128, 0)
DEST_COORD = (40 * 256 + 128, 40 * 256 + 128, 0)

STUB_TECHNO_TYPE = STUBS + 0x300
STUB_MARK = STUBS + 0x310
STUB_SET_LOCATION = STUBS + 0x320
STUB_SET_HEIGHT = STUBS + 0x330
STUB_MAP_COORDS = STUBS + 0x340
STUB_VT_18C = STUBS + 0x350
STUB_SET_DESTINATION = STUBS + 0x360
STUB_IDLE = STUBS + 0x370
STUB_ADDREF = STUBS + 0x380


def chrono_process_row(*, blocks=0, chrono_delay=60, stale_delay=0, start=1000,
                       frames=200):
    """Frames of a Chrono Warp from the first frame after Launch case 4.

    Each frame follows UnitClass::AI and FootClass::AI's control flow: the
    prologue's extra Process while WarpingIn (+0x271, vt+0x1D8 0x70C5C0) or
    BeingWarpedOut (+0x270, vt+0x1D4 0x70C5B0) with the latch
    (0x7362A7..0x7362F5); the frozen return while BeingWarpedOut
    (0x7362FB..0x73635A, no Temporal attacker); FootClass::AI's Process
    (0x4DA877) and its end of the piggyback when Is_Ok_To_End answers true
    (0x4DAE5F..0x4DAEC3). Process 0x7192F0, its TimerCheck 0x719BF0 and
    Is_Ok_To_End 0x719F30 run natively. Rows keep the frames whose state,
    owner bytes, timer or events changed."""
    emu = Emu()
    emu.write32(FRAME, start - 1)
    emu.write32(STASH, STASH_VT)
    emu.write32(STASH_VT + 4, STUB_ADDREF)
    emu.hook(STUB_ADDREF, lambda _e: 1, 4)
    emu.invoke(0x718000, ecx=TELEPORT)
    emu.invoke(0x55A710, args=[TELEPORT + 4, OWNER])
    emu.invoke(0x719E90, args=[TELEPORT + 0x18, STASH])
    emu.write32(OWNER, OWNER_VT)
    write8(emu, OWNER + 0x8C, 0)
    write8(emu, OWNER + 0x90, 1)
    write_coord(emu, OWNER + 0x9C, SOURCE_COORD)
    write8(emu, OWNER + 0x270, 0)
    write8(emu, OWNER + 0x271, 0)
    write8(emu, OWNER + 0x27C, 1)
    emu.write32(OWNER + 0x280, 0)
    emu.write32(OWNER + 0x284, stale_delay)
    write_coord(emu, OWNER + 0x288, DEST_COORD)
    emu.write32(OWNER + 0x2B4, 0)
    write8(emu, OWNER + 0x3D5, 1)
    emu.write32(OWNER + 0x428, 1)
    emu.write32(OWNER + 0x42C, HOUSE)
    write8(emu, OWNER + 0x6AD, 0)
    emu.write32(OWNER_TYPE + 0x574, -1)
    emu.write32(OWNER_TYPE + 0x578, -1)
    emu.write32(RULES + 0x218, CHRONO_IN_SOUND)
    emu.write32(RULES + 0x21C, CHRONO_OUT_SOUND)
    emu.write32(RULES + 0x33C, WARP_ANIM_TYPE)
    emu.write32(RULES + 0xBEC, chrono_delay)
    pending_blocks = [blocks]

    def owner_call(slot, stub, pops, answer):
        emu.write32(OWNER_VT + slot, stub)
        emu.hook(stub, answer, pops)

    def event(*fields):
        emu.events.append(list(fields))

    def set_location(e):
        coord = read_coord(e, e.arg(0))
        write_coord(e, OWNER + 0x9C, coord)
        event('set_location', coord)

    def map_coords(e):
        out = e.arg(0)
        x, y, _ = read_coord(e, OWNER + 0x9C)
        e.uc.mem_write(out, struct.pack('<hh', x // 256, y // 256))
        return out

    owner_call(0x84, STUB_TECHNO_TYPE, 0, lambda _e: OWNER_TYPE)
    owner_call(0x124, STUB_MARK, 4, lambda e: event('mark', e.arg(0)))
    owner_call(0x1B4, STUB_SET_LOCATION, 4, set_location)
    owner_call(0x1CC, STUB_SET_HEIGHT, 4, lambda e: event('set_height', i32(e.arg(0))))
    owner_call(0x1B8, STUB_MAP_COORDS, 4, map_coords)
    owner_call(0x18C, STUB_VT_18C, 4, lambda e: event('vt_18c', e.arg(0)))
    owner_call(0x480, STUB_SET_DESTINATION, 8,
               lambda e: event('set_destination', e.arg(0), e.arg(1) & 0xFF))
    owner_call(0x484, STUB_IDLE, 8, lambda e: event('idle', e.arg(0) & 0xFF, e.arg(1) & 0xFF))

    def anim(e):
        kind = 'warp' if e.arg(0) == WARP_ANIM_TYPE else hex(e.arg(0))
        event('anim', kind, read_coord(e, e.arg(1)), e.arg(2), e.arg(3), e.arg(4))
        return e.uc.reg_read(UC_X86_REG_ECX)

    def sound(e):
        event('sound', e.uc.reg_read(UC_X86_REG_ECX),
              read_coord(e, e.uc.reg_read(UC_X86_REG_EDX)))
        return 0

    def update_position(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != TELEPORT:
            raise OracleError('Update_Position on an unexpected object')
        coord = [i32(e.arg(0)), i32(e.arg(1)), i32(e.arg(2))]
        place = e.arg(3) & 0xFF
        event('update_position', coord, place)
        if place:
            write_coord(e, TELEPORT + 0x28, coord)
            return 1
        if pending_blocks[0]:
            pending_blocks[0] -= 1
            write_coord(e, OWNER + 0x288, [coord[0] + 256, coord[1], coord[2]])
            return 0
        return 1

    def validation(e):
        event('post_warp_validation', [i32(e.arg(0)), i32(e.arg(1)), i32(e.arg(2))])

    emu.hook(0x421EA0, anim, 0x1C)
    emu.hook(0x7509E0, sound, 4)
    emu.hook(0x718260, update_position, 0x10)
    emu.hook(0x578460, lambda _e: 1, 8)
    emu.hook(0x7187A0, validation, 0xC)
    emu.hook(0x70C610, lambda e: event('archive_target', e.arg(0)), 4)
    emu.hook(0x70F770, lambda _e: event('shorten_scan'), 0)

    def acquire(_e):
        event('passive_acquire')
        return 0

    emu.hook(0x709480, acquire, 0)

    def snapshot():
        return [emu.read_i32(TELEPORT + 0x38), read8(emu, OWNER + 0x270),
                read8(emu, OWNER + 0x271), read8(emu, OWNER + 0x27C),
                emu.read_i32(TELEPORT + 0x3C), emu.read_i32(TELEPORT + 0x44),
                emu.read_i32(OWNER + 0x284)]

    def process(caller):
        before = emu.read_i32(TELEPORT + 0x38)
        emu.events = []
        emu.invoke(0x7192F0, args=[TELEPORT + 4])
        return [caller, before, emu.read_i32(TELEPORT + 0x38), emu.events]

    rows = []
    last = None
    ended = None
    for frame in range(start, start + frames):
        emu.write32(FRAME, frame)
        calls = []
        if read8(emu, OWNER + 0x271) or (read8(emu, OWNER + 0x270)
                                        and read8(emu, OWNER + 0x27C)):
            calls.append(process('prologue'))
        if not read8(emu, OWNER + 0x270):
            calls.append(process('foot'))
            if emu.invoke(0x719F30, args=[TELEPORT + 0x18]) & 0xFF:
                ended = frame - start
        state = snapshot()
        timer_start = state[4] - start if state[4] != -1 else -1
        state = state[:4] + [timer_start] + state[5:]
        calls = [call for call in calls if call[1] != call[2] or call[3]]
        if calls or state != last or ended is not None:
            rows.append(dict(frame=frame - start, calls=calls, state=state[0],
                             warped_out=state[1], warping_in=state[2], latched=state[3],
                             timer=[state[4], state[5]], delay=state[6]))
        last = state
        if ended is not None:
            break
    if ended is None:
        raise OracleError('the warp did not end')
    return dict(blocks=blocks, chrono_delay=chrono_delay, stale_delay=stale_delay,
                ended=ended, frames=rows)


def chrono_process():
    return [chrono_process_row(),
            chrono_process_row(blocks=1),
            chrono_process_row(blocks=2),
            chrono_process_row(stale_delay=60),
            chrono_process_row(blocks=1, chrono_delay=0)]


# ---------------------------------------------------------------- chrono_update_position

CHRONO_CELLS = CHRONO + 0x10000
CHRONO_CELL_VT = CHRONO + 0x8000
CHRONO_OBJECTS = CHRONO + 0x20000
CHRONO_OBJECT_VT = CHRONO + 0x9000
CHRONO_OBJECT_TYPES = CHRONO + 0x30000
C4_WARHEAD = CHRONO + 0xA000
STUB_CELL_COORDS = STUBS + 0x390
STUB_OBJ_CURTAIN = STUBS + 0x3A0
STUB_OBJ_WHAT = STUBS + 0x3B0
STUB_OBJ_COORDS = STUBS + 0x3C0
STUB_OBJ_TYPE = STUBS + 0x3D0
STUB_OBJ_DAMAGE = STUBS + 0x3E0
STUB_PUT = STUBS + 0x3F0
STUB_REMOVE = STUBS + 0x400
BRIDGE_HEIGHT = 416  # 0xB0EC2C as its initializer 0x717F60 leaves it: four 104-lepton levels


def cell_of(coord):
    return (coord[0] // 256, coord[1] // 256)



class ChronoMap:
    """MapClass::operator[] by coordinate 0x565730 and by cell 0x5657A0,
    answering one fixture CellClass per cell: MapCoords (+0x24), no tube
    (+0x44 = -1), the row's flags (+0x140), the ground and bridge object
    lists (+0xE4, +0xE8) and GetCoords vt+0x48 (the centre raised 104 leptons
    per level)."""

    def __init__(self, emu, cells, heads=None):
        self.emu = emu
        self.cells = cells
        self.heads = heads or {}
        self.addresses = {}
        emu.write32(CHRONO_CELL_VT + 0x48, STUB_CELL_COORDS)
        emu.hook(0x565730, lambda e: self.address(cell_of(read_coord(e, e.arg(0)))), 4)
        emu.hook(0x5657A0, lambda e: self.address(read_cell(e, e.arg(0))), 4)
        emu.hook(STUB_CELL_COORDS, self.coords, 4)

    def address(self, cell):
        cell = tuple(cell)
        if cell not in self.addresses:
            emu = self.emu
            this = CHRONO_CELLS + 0x200 * len(self.addresses)
            facts = self.cells.get(cell, {})
            emu.write32(this, CHRONO_CELL_VT)
            emu.uc.mem_write(this + 0x24, struct.pack('<hh', *cell))
            emu.write32(this + 0x44, -1)
            emu.write32(this + 0x140, facts.get('flags', 0))
            emu.write32(this + 0xE4, self.heads.get((cell, 'ground', 'first'), 0))
            emu.write32(this + 0xE8, self.heads.get((cell, 'bridge', 'first'), 0))
            self.addresses[cell] = this
        return self.addresses[cell]

    def coords(self, emu):
        this = emu.uc.reg_read(UC_X86_REG_ECX)
        cell = next(cell for cell, address in self.addresses.items() if address == this)
        level = self.cells.get(cell, {}).get('level', 0)
        out = emu.arg(0)
        write_coord(emu, out, [cell[0] * 256 + 128, cell[1] * 256 + 128,
                               level * LEVEL_LEPTONS])
        return out


def chrono_cells(cells):
    return [[x, y, facts.get('level', 0), facts.get('flags', 0)]
            for (x, y), facts in sorted(cells.items())]


def update_position_row(*, place, coord, owner='unit', owner_coord=SOURCE_COORD,
                        marked=None, on_bridge=False, mz=0, cells=None, objects=(),
                        found=(41, 39), floor=0):
    """Update_Position 0x718260 on a fixture map. `cells` maps a cell to its
    level and flags; `objects` lists (name, cell, list, what, foot, curtained,
    coords, strength) in each cell list's order; `floor` answers the cell
    floor height 0x578080."""
    cells = dict(cells or {})
    emu = Emu()
    emu.write32(FRAME, 4000)
    emu.write32(0xB0EC2C, BRIDGE_HEIGHT)
    emu.invoke(0x718000, ecx=TELEPORT)
    emu.invoke(0x55A710, args=[TELEPORT + 4, OWNER])
    if marked is not None:
        write_coord(emu, TELEPORT + 0x28, marked)
    emu.write32(OWNER, OWNER_VT)
    write8(emu, OWNER + 0x8C, on_bridge)
    write_coord(emu, OWNER + 0x9C, owner_coord)
    write_coord(emu, OWNER + 0x288, coord)
    emu.write32(OWNER_TYPE + 0x5B4, mz)
    emu.write32(OWNER_TYPE + 0xA0, 300)
    emu.write32(RULES + 0xFA8, C4_WARHEAD)
    names = {OWNER: 'owner'}
    facts = {OWNER: dict(what=0xF if owner == 'infantry' else 1, coords=list(owner_coord),
                         curtained=False, type=OWNER_TYPE)}
    heads = {}
    for index, (name, cell, layer, what, foot, curtained, at, strength) in enumerate(objects):
        this = CHRONO_OBJECTS + 0x400 * index
        kind = CHRONO_OBJECT_TYPES + 0x100 * index
        emu.write32(this, CHRONO_OBJECT_VT)
        write8(emu, this + 0x14, 0x4 if foot else 0)
        emu.write32(this + 0x30, 0)
        emu.write32(kind + 0xA0, strength)
        names[this] = name
        facts[this] = dict(what=what, coords=list(at), curtained=curtained, type=kind)
        key = (tuple(cell), layer)
        if key in heads:
            emu.write32(heads[key] + 0x30, this)
        else:
            heads[(tuple(cell), layer, 'first')] = this
        heads[key] = this
    ChronoMap(emu, cells, heads)

    def fact(e, key):
        return facts[e.uc.reg_read(UC_X86_REG_ECX)][key]

    def obj_coords(e):
        out = e.arg(0)
        write_coord(e, out, fact(e, 'coords'))
        return out

    def damage(e):
        damage_value = e.read_i32(e.arg(0))
        e.events.append(['damage', names[e.uc.reg_read(UC_X86_REG_ECX)], damage_value,
                         i32(e.arg(1)), 'C4' if e.arg(2) == C4_WARHEAD else hex(e.arg(2)),
                         e.arg(3), e.arg(4) & 0xFF, e.arg(5) & 0xFF, e.arg(6)])
        return 0

    for vtable in (OWNER_VT, CHRONO_OBJECT_VT):
        emu.write32(vtable + 0x160, STUB_OBJ_CURTAIN)
        emu.write32(vtable + 0x2C, STUB_OBJ_WHAT)
        emu.write32(vtable + 0x48, STUB_OBJ_COORDS)
        emu.write32(vtable + 0x84, STUB_OBJ_TYPE)
        emu.write32(vtable + 0x16C, STUB_OBJ_DAMAGE)
    emu.write32(OWNER_VT + 0xF0, STUB_PUT)
    emu.write32(OWNER_VT + 0xF4, STUB_REMOVE)
    emu.hook(STUB_OBJ_CURTAIN, lambda e: int(fact(e, 'curtained')), 0)
    emu.hook(STUB_OBJ_WHAT, lambda e: fact(e, 'what'), 0)
    emu.hook(STUB_OBJ_COORDS, obj_coords, 4)
    emu.hook(STUB_OBJ_TYPE, lambda e: fact(e, 'type'), 0)
    emu.hook(STUB_OBJ_DAMAGE, damage, 0x1C)
    emu.hook(STUB_PUT, lambda e: e.events.append(['put', read_coord(e, e.arg(0))]), 4)
    emu.hook(STUB_REMOVE, lambda e: e.events.append(['remove', read_coord(e, e.arg(0))]), 4)

    def floor_height(e):
        e.events.append(['floor', read_coord(e, e.arg(0))])
        return floor

    def zone(e):
        e.events.append(['zone', read_cell(e, e.arg(0)), e.arg(1), e.arg(2) & 0xFF])
        return 7

    def nearby(e):
        args = [e.arg(index) for index in range(15)]
        e.events.append(['nearby', read_cell(e, args[1]), args[2], args[3], args[4],
                         args[5] & 0xFF, args[6], args[7], args[8] & 0xFF, args[9] & 0xFF,
                         args[10] & 0xFF, args[11] & 0xFF, read_cell(e, args[12]),
                         args[13] & 0xFF, args[14] & 0xFF])
        e.uc.mem_write(args[0], struct.pack('<hh', *found))
        return args[0]

    emu.hook(0x578080, floor_height, 4)
    emu.hook(0x56D230, zone, 0xC)
    emu.hook(0x56DC20, nearby, 0x3C)
    result = emu.invoke(0x718260, ecx=TELEPORT, args=[*map(u32_value, coord), int(place)]) & 0xFF
    return dict(place=place, coord=list(coord), owner=owner, owner_coord=list(owner_coord),
                marked=None if marked is None else list(marked), on_bridge=on_bridge, mz=mz,
                cells=chrono_cells(cells),
                objects=[[name, list(cell), layer, what, foot, curtained, list(at), strength]
                         for name, cell, layer, what, foot, curtained, at, strength in objects],
                found=list(found), floor=floor, result=result, events=emu.events,
                marked_after=read_coord(emu, TELEPORT + 0x28),
                destination_after=read_coord(emu, OWNER + 0x288),
                on_bridge_after=read8(emu, OWNER + 0x8C))


def u32_value(value):
    return value & 0xFFFFFFFF


def update_position():
    """Rows over a flat map around DEST_COORD's cell (40, 40). Each blocked
    row's `found` is the cell VERA's Find_Nearby_Passable_Cell port picks in
    the same world (chronosphere_tests rebuilds it); the native search
    itself is a stub here. `spot` is where VERA's spawn puts an
    infantryman in the cell."""
    dest = list(DEST_COORD)
    sub = [dest[0] + 30, dest[1] - 20, 215]
    spot = [dest[0] + 64, dest[1] - 64, 0]
    south = [40 * 256 + 128, 42 * 256 + 128, 0]

    def unit(name, cell=(40, 40), at=None, curtained=False, layer='ground'):
        return (name, cell, layer, 1, True, curtained, at or dest, 400)

    def infantry(name, at, cell=(40, 40)):
        return (name, cell, 'ground', 0xF, True, False, at, 125)

    def building(cell, at):
        return ('GAPOWR', cell, 'ground', 6, False, False, at, 750)

    rows = [
        # Placing: Marked empty, then set; the bridge height enters only
        # when the owner was not already on the bridge.
        update_position_row(place=True, coord=sub, floor=208, cells={(40, 40): {'level': 2}}),
        update_position_row(place=True, coord=sub, marked=[100, 200, 0], floor=208,
                            cells={(40, 40): {'level': 2, 'flags': 0x100}}),
        update_position_row(place=True, coord=sub, marked=[100, 200, 0], floor=208,
                            on_bridge=True, cells={(40, 40): {'level': 2, 'flags': 0x100}}),
        update_position_row(place=True, coord=sub, floor=0, on_bridge=True),
        # Testing: an empty cell; Foot victims; the Iron Curtain; a building.
        update_position_row(place=False, coord=dest),
        update_position_row(place=False, coord=dest, marked=[100, 200, 0]),
        update_position_row(place=False, coord=dest, objects=[unit('HTNK')]),
        update_position_row(place=False, coord=dest, objects=[unit('HTNK', curtained=True)]),
        update_position_row(place=False, coord=dest,
                            objects=[unit('HTNK', curtained=True), unit('MTNK')]),
        update_position_row(place=False, coord=dest, objects=[building((40, 40), dest)],
                            found=(40, 39)),
        # Blocked away from the cell centre, onto a raised cell: the found
        # cell keeps the offset from the blocked cell's coordinate.
        update_position_row(place=False, coord=[south[0] + 30, south[1] - 20, 0],
                            objects=[building((40, 42), south)], found=(40, 41),
                            cells={(40, 41): {'level': 1}}),
        update_position_row(place=False, coord=[south[0] + 30, south[1] - 20, 215],
                            objects=[building((40, 42), south)], found=(41, 41),
                            cells={(40, 42): {'level': 2}, (41, 39): {'level': 1}}),
        # Infantry: killed only at the warping infantryman's exact coordinate.
        update_position_row(place=False, coord=spot, owner='infantry',
                            objects=[infantry('E1', spot)]),
        update_position_row(place=False, coord=dest, owner='infantry',
                            objects=[infantry('E1', spot)]),
        update_position_row(place=False, coord=dest, owner='infantry',
                            objects=[infantry('E1', spot), unit('HTNK')]),
        update_position_row(place=False, coord=dest, objects=[infantry('E1', spot)]),
        # Bridges: no deck flag blocks; the bridge list is walked instead.
        update_position_row(place=False, coord=dest, cells={(40, 40): {'flags': 0x100}}),
        update_position_row(place=False, coord=dest, cells={(40, 40): {'flags': 0x300}},
                            objects=[unit('HTNK', layer='bridge'), unit('MTNK')]),
    ]
    # The MovementZone handed to the nearby-cell search.
    for mz in range(1, 13):
        rows.append(update_position_row(place=False, coord=dest, mz=mz,
                                        objects=[building((40, 40), dest)]))
    return rows


# ---------------------------------------------------------------- chrono_destination

CASE4_OFFSET = CHRONO + 0xB000
CASE4_PCELL = CHRONO + 0xB010
CASE4_SUPER = CHRONO + 0xB100
CASE4_OBJECT = CHRONO + 0xC000
CASE4_OBJECT_VT = CHRONO + 0xD000
STUB_CASE4_WHAT = STUBS + 0x410
STUB_CASE4_COORDS = STUBS + 0x420


def chrono_destination_row(*, what, offset, coords, src=(21, 21), target=(40, 40),
                           cells=None):
    """Launch case 4's `+0x288` for one object of the source block: the
    Unit's (0x6CC9AF..0x6CCA4C) and then the others' (0x6CCB6A..0x6CCC2D),
    run as slices on Launch's frame (the offset entry at [esp+0x2C], the
    clicked cell at [esp+0x1E8], the Super at [esp+0x44]). The bridge height
    0xB0C07C holds 416, as its initializer 0x6CAD80 leaves it."""
    cells = dict(cells or {})
    emu = Emu()
    emu.write32(0xB0C07C, BRIDGE_HEIGHT)
    ChronoMap(emu, cells)
    emu.uc.mem_write(CASE4_OFFSET, struct.pack('<hh', *offset))
    emu.uc.mem_write(CASE4_PCELL, struct.pack('<hh', *target))
    emu.uc.mem_write(CASE4_SUPER + 0x62, struct.pack('<hh', *src))
    emu.write32(CASE4_OBJECT, CASE4_OBJECT_VT)
    emu.write32(CASE4_OBJECT_VT + 0x2C, STUB_CASE4_WHAT)
    emu.write32(CASE4_OBJECT_VT + 0x48, STUB_CASE4_COORDS)
    emu.hook(STUB_CASE4_WHAT, lambda _e: what, 0)
    emu.hook(STUB_CASE4_COORDS, coords_stub(coords), 4)
    uc = emu.uc
    sp = STACK_BASE + STACK_SIZE - 0x1000
    emu.write32(sp + 0x2C, CASE4_OFFSET)
    emu.write32(sp + 0x44, CASE4_SUPER)
    emu.write32(sp + 0x1E8, CASE4_PCELL)
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_ESI, CASE4_OBJECT)
    uc.reg_write(UC_X86_REG_EBP, 0)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    run_checked(uc, 0x6CC9AF, 0x6CCA4C, count=10_000)
    run_checked(uc, 0x6CCB6A, 0x6CCC2D, count=10_000)
    return dict(what=what, offset=list(offset), coords=list(coords), src=list(src),
                target=list(target), cells=chrono_cells(cells),
                destination=read_coord(emu, sp + 0x10))


def chrono_destination():
    """Objects of a source block at (21, 21) warped to (40, 40); `spot` is
    where VERA's spawn puts an infantryman in a cell."""
    def centre(cell, z=0):
        return [cell[0] * 256 + 128, cell[1] * 256 + 128, z]

    def spot(cell, z=0):
        x, y, _ = centre(cell)
        return [x + 64, y - 64, z]

    unit, infantry = 1, 0xF
    return [
        chrono_destination_row(what=unit, offset=(0, 0), coords=centre((21, 21))),
        chrono_destination_row(what=unit, offset=(-1, -1), coords=centre((20, 20)),
                               cells={(39, 39): {'level': 1}}),
        chrono_destination_row(what=unit, offset=(1, 0), coords=centre((22, 21)),
                               cells={(41, 40): {'flags': 0x100}}),
        chrono_destination_row(what=unit, offset=(0, 1), coords=centre((21, 22), 104),
                               cells={(21, 22): {'level': 1}, (40, 41): {'level': 2}}),
        chrono_destination_row(what=infantry, offset=(0, 0), coords=spot((21, 21)),
                               cells={(40, 40): {'level': 2}}),
        chrono_destination_row(what=infantry, offset=(-1, 0), coords=spot((20, 21), 104),
                               cells={(20, 21): {'level': 1}}),
        chrono_destination_row(what=infantry, offset=(-1, 0), coords=spot((20, 21)),
                               cells={(21, 21): {'level': 3}, (40, 40): {'level': 4}}),
        chrono_destination_row(what=infantry, offset=(1, 1), coords=spot((22, 22)),
                               cells={(41, 41): {'flags': 0x100}}),
    ]

# ---------------------------------------------------------------- psychic dominator

# The Psychic Dominator's globals (SuperWeaponEffects, reset by 0x539740 and
# saved by 0x539890): its cell, status, anim and owner.
G_PSYDOM_COORDS = 0xA9FA48
G_PSYDOM_STATUS = 0xA9FAC0
G_PSYDOM_ANIM = 0xA9FAC4
G_PSYDOM_OWNER = 0xA9FAC8
G_NUKE_FLASH = 0xA9FABC
G_CHRONO_SCREEN = 0xA9FAB0
G_STORM_ACTIVE = 0xA9FAB4
PSYDOM = BASE + 0x380000
PSYDOM_ANIM = PSYDOM
PSYDOM_ANIM_TYPE = PSYDOM + 0x1000
PSYDOM_ANIM_TYPE_VT = PSYDOM + 0x2000
PSYDOM_IMAGE = PSYDOM + 0x3000
PSYDOM_FIRST_ANIM = PSYDOM + 0x4000
PSYDOM_SECOND_ANIM = PSYDOM + 0x5000
PSYDOM_HOUSE = PSYDOM + 0x6000
STUB_PSYDOM_IMAGE = STUBS + 0x500
# ScenarioClass fields: Timer_1248 (start, pad, duration), the ambient
# target +0x3530 and current +0x352C, the profiles and the two fade rates.
SCN_TIMER = 0x1248
SCN_TARGET = 0x3530
SCN_CURRENT = 0x352C
SCN_CELL_REDRAW = 0x34AB
SCN_PROFILES = {'ambient': 0x3528, 'ion': (0x3548, 0x354C, 0x3550, 0x3554),
                'nuke': (0x3560, 0x3564, 0x3568, 0x356C),
                'dominator': (0x357C, 0x3580, 0x3584, 0x3588)}
SCN_NUKE_RATE = 0x3578
SCN_DOMINATOR_RATE = 0x3594


def psydom_process_emu():
    """One emulator for many PsychicDominator::Process 0x53AF40 calls: its
    anim's type answers GetImage (vt+0x9C) with a fixture image whose frame
    count (+6) each step writes; MindControlArea 0x53B080 and UpdateLighting
    0x53C280 are recorded stubs."""
    emu = Emu()
    emu.write32(PSYDOM_ANIM + 0xC8, PSYDOM_ANIM_TYPE)
    emu.write32(PSYDOM_ANIM_TYPE, PSYDOM_ANIM_TYPE_VT)
    emu.write32(PSYDOM_ANIM_TYPE_VT + 0x9C, STUB_PSYDOM_IMAGE)

    def image(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != PSYDOM_ANIM_TYPE:
            raise OracleError('GetImage on an unexpected type')
        return PSYDOM_IMAGE

    emu.hook(STUB_PSYDOM_IMAGE, image, 0)
    emu.hook(0x53B080, lambda e: e.events.append(['mind_control_area']), 0)
    emu.hook(0x53C280, lambda e: e.events.append(['update_lighting']), 0)
    return emu


def psydom_process_step(emu, *, status, stage=0, frames=0, percent=50, ambient=(100, 100)):
    emu.events = []
    emu.write32(G_PSYDOM_STATUS, status)
    emu.write32(G_PSYDOM_ANIM, PSYDOM_ANIM)
    emu.uc.mem_write(G_PSYDOM_COORDS, struct.pack('<hh', 33, 44))
    emu.write32(PSYDOM_ANIM + 0xAC, stage)
    emu.uc.mem_write(PSYDOM_IMAGE + 6, struct.pack('<h', frames))
    emu.write32(RULES + 0x304, percent)
    emu.write32(SCENARIO + SCN_TARGET, ambient[0])
    emu.write32(SCENARIO + SCN_CURRENT, ambient[1])
    emu.invoke(0x53AF40)
    return dict(status=status, stage=stage, frames=frames, percent=percent,
                ambient=list(ambient), status_after=emu.read_i32(G_PSYDOM_STATUS),
                anim_after=int(emu.read32(G_PSYDOM_ANIM) != 0),
                coords_after=read_cell(emu, G_PSYDOM_COORDS), events=list(emu.events))


PSYDOM_PERCENTS = list(range(0, 101)) + [-10, -1, 101, 150, 1000]
PSYDOM_FRAMES = list(range(1, 65))


def psydom_fire_stages():
    """Status 2's test (0x53AF64..0x53AFAA): for each DominatorFireAtPercentage
    (Rules+0x304) and anim frame count, the first stage (+0xAC) whose
    FILD/FIDIV ratio is at least FILD percent FMUL 0.01, searched in
    0..2*frames (the test is monotonic in the stage); None if none is."""
    emu = psydom_process_emu()
    rows = []
    for percent in PSYDOM_PERCENTS:
        firsts = []
        for frames in PSYDOM_FRAMES:
            def fires(stage):
                step = psydom_process_step(emu, status=2, stage=stage, frames=frames,
                                           percent=percent)
                fired = step['status_after'] == 3
                if fired != (step['events'] == [['mind_control_area']]):
                    raise OracleError('status 3 without MindControlArea')
                return fired
            low, high = 0, 2 * frames + 1
            while low < high:
                middle = (low + high) // 2
                if fires(middle):
                    high = middle
                else:
                    low = middle + 1
            firsts.append(low if low <= 2 * frames else None)
        rows.append([percent, firsts])
    return dict(frames=PSYDOM_FRAMES, first_stage=rows)


def psydom_process():
    """Single steps of every status, the status 2 rows at the retail
    PDFXCLD count (60 frames, 20 percent) and a zero frame count."""
    emu = psydom_process_emu()
    step = lambda **row: psydom_process_step(emu, **row)
    rows = [step(status=0), step(status=6), step(status=-1), step(status=1)]
    for stage in (11, 12, 13):
        rows.append(step(status=2, stage=stage, frames=60, percent=20))
    rows += [step(status=2, stage=0, frames=0, percent=20),
             step(status=2, stage=5, frames=0, percent=20)]
    for stage in (10, 11, 21, 30):
        rows.append(step(status=3, stage=stage, frames=21))
    for stage in (19, 20, 21, 30):
        rows.append(step(status=4, stage=stage, frames=21))
    rows += [step(status=5, ambient=(100, 100)), step(status=5, ambient=(100, 120)),
             step(status=5, ambient=(150, 140))]
    return dict(steps=rows, fire_stages=psydom_fire_stages())


def psydom_start_row(*, first=True, second=True, frame=4000, cell=(33, 44), level=0):
    """PsyDom::Start 0x53AE50 (ECX the house, the cell by value; RET 4) with
    Rules DominatorFirstAnim/SecondAnim (+0x2FC/+0x300) set or null: the
    globals, the anim constructor 0x421EA0's arguments (a recorded stub),
    Timer_1248 and UpdateLighting 0x53C280 (a recorded stub)."""
    emu = Emu()
    emu.write32(FRAME, frame)
    emu.write32(RULES + 0x2FC, PSYDOM_FIRST_ANIM if first else 0)
    emu.write32(RULES + 0x300, PSYDOM_SECOND_ANIM if second else 0)
    emu.write32(SCENARIO + SCN_TIMER, 77)
    emu.write32(SCENARIO + SCN_TIMER + 8, 55)
    Cells(emu, {tuple(cell): level})

    def anim(e):
        kind = {PSYDOM_FIRST_ANIM: 'first', PSYDOM_SECOND_ANIM: 'second'}.get(e.arg(0),
                                                                             hex(e.arg(0)))
        e.events.append(['anim', kind, read_coord(e, e.arg(1)), i32(e.arg(2)), i32(e.arg(3)),
                         e.arg(4), i32(e.arg(5)), e.arg(6) & 0xFF])
        return e.uc.reg_read(UC_X86_REG_ECX)

    emu.hook(0x421EA0, anim, 0x1C)
    emu.hook(0x53C280, lambda e: e.events.append(['update_lighting']), 0)
    packed = struct.unpack('<I', struct.pack('<hh', *cell))[0]
    emu.invoke(0x53AE50, ecx=PSYDOM_HOUSE, args=[packed])
    return dict(first=first, second=second, frame=frame, cell=list(cell), level=level,
                status=emu.read_i32(G_PSYDOM_STATUS),
                owner_set=int(emu.read32(G_PSYDOM_OWNER) == PSYDOM_HOUSE),
                anim_set=int(emu.read32(G_PSYDOM_ANIM) != 0),
                coords=read_cell(emu, G_PSYDOM_COORDS),
                timer=[emu.read_i32(SCENARIO + SCN_TIMER), emu.read_i32(SCENARIO + SCN_TIMER + 8)],
                events=emu.events)


def psydom_start():
    return [psydom_start_row(), psydom_start_row(level=2, cell=(70, 12), frame=9),
            psydom_start_row(first=False), psydom_start_row(second=False)]


def update_lighting_row(*, nuke=0, chrono=0, storm=False, psydom=0):
    """ScenarioClass::UpdateLighting 0x53C280: the ambient target it writes
    (+0x3530) and RecalcLighting 0x53AD00's arguments (ECX, EDX and two stack
    words; a recorded stub)."""
    emu = Emu()
    emu.write32(G_NUKE_FLASH, nuke)
    emu.write32(G_CHRONO_SCREEN, chrono)
    write8(emu, G_STORM_ACTIVE, storm)
    emu.write32(G_PSYDOM_STATUS, psydom)
    emu.write32(SCENARIO + SCN_PROFILES['ambient'], 101)
    for name, values in (('ion', (87, 30, 40, 75)), ('nuke', (200, 175, 150, 125)),
                         ('dominator', (150, 85, 20, 30))):
        for offset, value in zip(SCN_PROFILES[name], values):
            emu.write32(SCENARIO + offset, value)

    def recalc(e):
        e.events.append(['recalc', i32(e.uc.reg_read(UC_X86_REG_ECX)),
                         i32(e.uc.reg_read(UC_X86_REG_EDX)), i32(e.arg(0)), i32(e.arg(1))])

    emu.hook(0x53AD00, recalc, 8)
    emu.invoke(0x53C280)
    return dict(nuke=nuke, chrono=chrono, storm=storm, psydom=psydom,
                target=emu.read_i32(SCENARIO + SCN_TARGET), events=emu.events)


def update_lighting():
    return [update_lighting_row(nuke=nuke, chrono=chrono, storm=storm, psydom=psydom)
            for nuke in (0, 1, 2) for chrono in (0, 1) for storm in (False, True)
            for psydom in (0, 1, 2, 3, 4, 5, 6)]


def ambient_step_row(*, target=150, current=100, rate=0.2, step=0.2, frame=1000,
                     timer=(999, 1), nuke=0, chrono=0, psydom=0, nuke_rate=3,
                     dominator_rate=1):
    """LogicClass::PerTickUpdate's ambient fade (0x55B33D..0x55B4D7) as a
    slice (EBP the Scenario, EBX the Rules, EDI the frame): the gates, the
    interval each lighting state selects for Timer_1248, the target clamp and
    the clamped step. NukeFlash::IsFadingIn/Out 0x53A110/0x53A120,
    ChronoScreenEffect::Active 0x53BAD0 and PsyDom::Active 0x53B400 run
    natively on their globals; 0x4AE4C0 and 0x4F42F0 are recorded stubs."""
    emu = Emu()
    emu.write32(FRAME, frame)
    emu.write32(G_NUKE_FLASH, nuke)
    emu.write32(G_CHRONO_SCREEN, chrono)
    emu.write32(G_PSYDOM_STATUS, psydom)
    emu.uc.mem_write(RULES + 0x1668, struct.pack('<d', rate))
    emu.uc.mem_write(RULES + 0x1670, struct.pack('<d', step))
    emu.write32(SCENARIO + SCN_TIMER, timer[0])
    emu.write32(SCENARIO + SCN_TIMER + 8, timer[1])
    emu.write32(SCENARIO + SCN_TARGET, target)
    emu.write32(SCENARIO + SCN_CURRENT, current)
    emu.write32(SCENARIO + SCN_NUKE_RATE, nuke_rate)
    emu.write32(SCENARIO + SCN_DOMINATOR_RATE, dominator_rate)
    write8(emu, SCENARIO + SCN_CELL_REDRAW, 0)
    emu.hook(0x4AE4C0, lambda e: e.events.append(['cell_lighting']), 0)
    emu.hook(0x4F42F0, lambda e: e.events.append(['redraw', e.arg(0)]), 4)
    uc = emu.uc
    sp = STACK_BASE + STACK_SIZE - 0x1000
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_EBP, SCENARIO)
    uc.reg_write(UC_X86_REG_EBX, RULES)
    uc.reg_write(UC_X86_REG_EDI, frame)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    run_checked(uc, 0x55B33D, 0x55B4D7, count=10_000)
    return dict(target=target, current=current, rate=rate, step=step, frame=frame,
                timer=list(timer), nuke=nuke, chrono=chrono, psydom=psydom,
                nuke_rate=nuke_rate, dominator_rate=dominator_rate,
                target_after=emu.read_i32(SCENARIO + SCN_TARGET),
                current_after=emu.read_i32(SCENARIO + SCN_CURRENT),
                timer_after=[emu.read_i32(SCENARIO + SCN_TIMER),
                             emu.read_i32(SCENARIO + SCN_TIMER + 8)],
                cell_redraw=read8(emu, SCENARIO + SCN_CELL_REDRAW), events=emu.events)


def ambient_step():
    row = ambient_step_row
    rows = [row(), row(psydom=1), row(psydom=4, dominator_rate=7), row(psydom=5, target=100,
                                                                       current=150),
            row(nuke=1), row(nuke=2), row(chrono=1), row(nuke=1, psydom=2),
            row(timer=(995, 6)), row(timer=(994, 6)), row(timer=(-1, 0)), row(timer=(-1, 3)),
            row(target=100), row(rate=0.0), row(target=-5, current=10), row(target=105),
            row(target=95, current=100), row(step=0.07), row(step=0.29), row(step=0.57),
            row(rate=0.01), row(rate=0.0011), row(psydom=3, dominator_rate=0),
            row(psydom=3, dominator_rate=-4)]
    return rows


# ScenarioClass::Read_INI_Basic 0x689E90's Dominator keys: (key, Scenario
# offset, the default-inverse slice, the conversion slice). Each default slice
# ends after FSTP double [ESP]; each conversion slice starts at the FMUL after
# ReadDouble returns and ends after Math__ftol.
DOMINATOR_LIGHTING_SITES = (
    ('DominatorAmbient', 0x357C, (0x68AAFD, 0x68AB17), (0x68AB26, 0x68AB37)),
    ('DominatorRed', 0x3580, (0x68AB37, 0x68AB51), (0x68AB60, 0x68AB71)),
    ('DominatorGreen', 0x3584, (0x68AB71, 0x68AB8B), (0x68AB9A, 0x68ABAB)),
    ('DominatorBlue', 0x3588, (0x68ABAB, 0x68ABC5), (0x68ABD4, 0x68ABE5)),
    ('DominatorGround', 0x358C, (0x68ABE5, 0x68ABFF), (0x68AC0E, 0x68AC1F)),
    ('DominatorLevel', 0x3590, (0x68AC1F, 0x68AC39), (0x68AC48, 0x68AC59)),
    ('DominatorAmbientChangeRate', 0x3594, (0x68AC59, 0x68AC73), (0x68AC82, 0x68AC93)),
)
DOMINATOR_PERCENT_TOKENS = ('0', '1.5', '.85', '.2', '.3', '1', '.01', '.009', '.0099',
                            '1.99999', '-.2', '.155', '2.5')
DOMINATOR_MILLI_TOKENS = ('0', '.001', '.0015', '.0009', '.002', '.05', '-.001', '.0319',
                          '1.99999', '.000989')
TRAMPOLINE = STUBS + 0x600
TRAMPOLINE_VALUE = STUBS + 0x680


def scenario_lighting_defaults(emu):
    """ScenarioClass::Set_Defaults 0x683610's lighting block
    0x683915..0x6839BD with EBP the Scenario, EBX zero (as at 0x68365A) and
    EAX 100 (as at 0x6838C2)."""
    uc = emu.uc
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    uc.reg_write(UC_X86_REG_ESP, STACK_BASE + STACK_SIZE - 0x1000)
    uc.reg_write(UC_X86_REG_EBP, SCENARIO)
    uc.reg_write(UC_X86_REG_EBX, 0)
    uc.reg_write(UC_X86_REG_EAX, 100)
    run_checked(uc, 0x683915, 0x6839BD, count=100)


def converted(emu, value, start, end):
    """A conversion slice of Read_INI_Basic on ReadDouble's answer `value`:
    a trampoline loads it into ST0 (FLD qword) and jumps to `start`; EAX at
    `end`."""
    uc = emu.uc
    uc.mem_write(TRAMPOLINE_VALUE, struct.pack('<d', value))
    jump = start - (TRAMPOLINE + 11)
    uc.mem_write(TRAMPOLINE, b'\xdd\x05' + u32(TRAMPOLINE_VALUE) + b'\xe9'
                 + struct.pack('<i', jump))
    uc.reg_write(UC_X86_REG_ESP, STACK_BASE + STACK_SIZE - 0x1000)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    run_checked(uc, TRAMPOLINE, end, count=500)
    return i32(uc.reg_read(UC_X86_REG_EAX))


def dominator_lighting_read():
    """The map's Dominator lighting. Set_Defaults' lighting block
    ([`scenario_lighting_defaults`]) writes the defaults. For each key,
    Read_INI_Basic's default slice turns the stored value into ReadDouble's
    default (its double at [ESP]), and its conversion slice turns
    ReadDouble's answer into the stored value ([`converted`], from the slice's
    FMUL). The answers are the defaults (a missing key) and each token's float
    scan widened to double."""
    emu = Emu()
    uc = emu.uc
    sp = STACK_BASE + STACK_SIZE - 0x1000
    scenario_lighting_defaults(emu)
    defaults = {name: emu.read_i32(SCENARIO + offset)
                for name, offset, _default, _convert in DOMINATOR_LIGHTING_SITES}

    def convert(value, start, end):
        return converted(emu, value, start, end)

    rows = []
    for name, offset, (default_start, default_end), (start, end) in DOMINATOR_LIGHTING_SITES:
        uc.reg_write(UC_X86_REG_ESP, sp)
        uc.reg_write(UC_X86_REG_ESI, SCENARIO)
        uc.reg_write(UC_X86_REG_EDI, 0)
        uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        run_checked(uc, default_start, default_end, count=50)
        default = struct.unpack('<d', uc.mem_read(uc.reg_read(UC_X86_REG_ESP), 8))[0]
        tokens = DOMINATOR_MILLI_TOKENS if offset >= 0x358C else DOMINATOR_PERCENT_TOKENS
        authored = []
        for token in tokens:
            value = struct.unpack('<f', struct.pack('<f', float(token)))[0]
            authored.append([token, convert(value, start, end)])
        rows.append(dict(key=name, offset=offset, stored_default=defaults[name],
                         default_double=default,
                         default_units=convert(default, start, end), authored=authored))
    return rows


# Map-authored Ground/Level stand-ins (Scenario offset, value), distinct per
# profile so each arm's reads show; NukeGround/NukeLevel (+0x3570/+0x3574)
# keep the values Set_Defaults writes, which no INI key changes.
RELIGHT_GROUND_LEVEL = ((0x3540, 21), (0x3544, 13), (0x3558, 30), (0x355C, 40),
                        (0x358C, 3), (0x3590, 7))
RELIGHT_CELL = CELL
RELIGHT_SCALARS = CELL + 0x800


def relight_row(*, storm=False, psydom=0, nuke=0, level=0, ambient=1000):
    """CellClass::ProcessColourComponents 0x484180's profile arms as a slice
    (0x48445F..0x4845A2): the gathered additive ([ESP+0x44], zero here: no
    light reaches the cell) joins the ambient the top holds (written at
    0x4841DE), both scalars start from the sum, then LightningStorm::IsActive 0x53A100, PsyDom::Active 0x53B400 and
    NukeFlash::IsFadingIn 0x53A110 pick the Ground/Level each scalar adds for
    the cell's level (+0x11B). The Scenario holds Set_Defaults' block
    (0x683915..0x6839BD) with RELIGHT_GROUND_LEVEL over it."""
    emu = Emu()
    uc = emu.uc
    sp = STACK_BASE + STACK_SIZE - 0x1000
    scenario_lighting_defaults(emu)
    for offset, value in RELIGHT_GROUND_LEVEL:
        emu.write32(SCENARIO + offset, value)
    write8(emu, G_STORM_ACTIVE, storm)
    emu.write32(G_PSYDOM_STATUS, psydom)
    emu.write32(G_NUKE_FLASH, nuke)
    write8(emu, RELIGHT_CELL + 0x11B, level & 0xFF)
    top, bottom, additive = RELIGHT_SCALARS, RELIGHT_SCALARS + 4, RELIGHT_SCALARS + 8
    emu.write32(top, ambient)
    emu.write32(bottom, 0)
    emu.write32(additive, 0)
    emu.write32(sp + 0x44, additive)
    emu.write32(sp + 0x50, bottom)
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_EBX, top)
    uc.reg_write(UC_X86_REG_EDI, RELIGHT_CELL)
    run_checked(uc, 0x48445F, 0x4845A2, count=200)
    return dict(storm=storm, psydom=psydom, nuke=nuke, level=level, ambient=ambient,
                nuke_ground=emu.read_i32(SCENARIO + 0x3570),
                nuke_level=emu.read_i32(SCENARIO + 0x3574), top=emu.read_i32(top),
                bottom=emu.read_i32(bottom))


def relight():
    rows = [relight_row(psydom=psydom, level=level)
            for psydom in (0, 1, 2, 3, 4, 5) for level in (0, 2, 7)]
    rows += [relight_row(storm=True, psydom=3, level=2),
             relight_row(nuke=1, level=2), relight_row(nuke=1, psydom=3, level=2),
             relight_row(psydom=3, level=4, ambient=1500)]
    return rows


# ---------------------------------------------------------------- spy plane

TYPE_SPY_PLANE = 8
AIRCRAFT_TYPE_ITEMS = 0xA8B21C
SCENARIO_INIT = 0xA8E7AC
# The local client's selected Super (VERA: `super_selection`); the player's
# Launch tails write -1 to it.
SELECTED_SUPER = 0x8809A0
DUMMY_CELL = 0xABDC50
SPY = BASE + 0x390000
SPY_CELL = SPY
SPY_TARGET = SPY + 0x100
SPY_NAV_COM = SPY + 0x200
SPY_DESTINATION_CELL = SPY + 0x300
SPY_TEAM = SPY + 0x400
SPY_TYPE = SPY + 0x1000
SPY_TYPE_VT = SPY + 0x2000
SPY_TYPE_ITEMS = SPY + 0x3000
SPY_PLANE_VT = SPY + 0x4000
SPY_WEAPON = SPY + 0x5000
SPY_WEAPON_TYPE = SPY + 0x6000
SPY_PLANES = SPY + 0x10000
SPY_PLANE_STRIDE = 0x1000
SPY_TYPE_INDEX = 5
# The fixture plane's Location (and so its cell, 40, 40) for the missions.
SPY_LOCATION = (40 * 256 + 128, 40 * 256 + 128, 1500)
# The removal rows' Map Size height (MapClass+0xF8), with PLAYFIELD's width.
SPY_SIZE_HEIGHT = 46

STUB_SPY_CREATE = STUBS + 0x700
STUB_SPY_QUEUE = STUBS + 0x710
STUB_SPY_DESTINATION = STUBS + 0x720
STUB_SPY_TARGET = STUBS + 0x730
STUB_SPY_UNLIMBO = STUBS + 0x740
STUB_SPY_COMMENCE = STUBS + 0x750
STUB_SPY_DELETE = STUBS + 0x760
STUB_SPY_WEAPON = STUBS + 0x770
STUB_SPY_TECHNO_TYPE = STUBS + 0x780
STUB_SPY_UNINIT = STUBS + 0x790


def spy_name(address):
    names = {0: 'null', SPY_TARGET: 'target', SPY_DESTINATION_CELL: 'edge_cell',
             DUMMY_CELL: 'dummy'}
    return names.get(address, hex(address))


def spy_plane_launch_row(*, charged=True, type_index=SPY_TYPE_INDEX, cell='real',
                         counts=(1, 1), player=True):
    """Launch 0x6CC390 from its entry for a type whose Type= (+0xB4) is 8:
    case 8 (0x6CD66F..0x6CD70B), the player's selection write (0x6CD6F8) and
    the shared EVA tail (0x6CD51E)."""
    emu = Emu()
    emu.write32(SUPER + 0x28, SW_TYPE)
    emu.write32(SUPER + 0x2C, HOUSE)
    emu.write32(SW_TYPE + 0xB4, TYPE_SPY_PLANE)
    write8(emu, SUPER + 0x6F, charged)
    emu.uc.mem_write(SPY_CELL, struct.pack('<hh', 40, 40))
    emu.write32(RULES + 0xC4C, counts[0])
    emu.write32(RULES + 0xC68, counts[1])
    emu.write32(SELECTED_SUPER, 9)
    found = {'null': 0, 'dummy': DUMMY_CELL, 'real': SPY_TARGET}[cell]

    def find_type(e):
        e.events.append(['find_aircraft_type',
                         read_name(e, e.uc.reg_read(UC_X86_REG_ECX))])
        return type_index

    def lookup(e):
        e.events.append(['map_cell', read_cell(e, e.arg(0))])
        return found

    def send(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != HOUSE:
            raise OracleError('SendSpyPlanes on another house')
        e.events.append(['send_spy_planes', i32(e.uc.reg_read(UC_X86_REG_EDX)), e.arg(0),
                         e.arg(1), spy_name(e.arg(2)), spy_name(e.arg(3))])
        return 1

    def vox_find(e):
        e.events.append(['vox_find', read_name(e, e.uc.reg_read(UC_X86_REG_ECX))])
        return 33

    emu.hook(0x41CAA0, find_type, 0)
    emu.hook(0x5657A0, lookup, 4)
    emu.hook(0x65EAB0, send, 0x10)
    emu.hook(0x753250, vox_find, 0)
    emu.hook(0x752A40, lambda e: e.events.append(
        ['vox_remove', e.uc.reg_read(UC_X86_REG_ECX)]), 0)
    emu.invoke(0x6CC390, ecx=SUPER, args=[SPY_CELL, int(player)])
    return dict(charged=charged, type_index=type_index, cell=cell, counts=list(counts),
                player=player, events=emu.events,
                selected_super=emu.read_i32(SELECTED_SUPER))


def spy_plane_launch():
    rows = [spy_plane_launch_row(charged=False)]
    for cell in ('null', 'dummy', 'real'):
        for type_index in (-1, SPY_TYPE_INDEX):
            for player in (False, True):
                rows.append(spy_plane_launch_row(cell=cell, type_index=type_index,
                                                 player=player))
    for counts in ((0, 0), (1, 2), (2, 1), (2, 2), (3, 3)):
        rows.append(spy_plane_launch_row(counts=counts, player=False))
    return rows


def send_spy_planes_row(*, count=1, edge=-1, waypoint_edge=0, picks=((30, 1),),
                        created=None, unlimbo=None):
    """HouseClass::SendSpyPlanes 0x65EAB0 with case 8's arguments (mission
    0x1E, the clicked cell as Target, no destination)."""
    emu = Emu()
    emu.write32(AIRCRAFT_TYPE_ITEMS, SPY_TYPE_ITEMS)
    emu.write32(SPY_TYPE_ITEMS + 4 * SPY_TYPE_INDEX, SPY_TYPE)
    emu.write32(SPY_TYPE, SPY_TYPE_VT)
    emu.write32(SPY_TYPE_VT + 0x8C, STUB_SPY_CREATE)
    emu.write32(HOUSE + 0x1E0, edge)
    emu.write32(HOUSE + 0x577C, waypoint_edge)
    for slot, stub in ((0x1E8, STUB_SPY_QUEUE), (0x480, STUB_SPY_DESTINATION),
                       (0x3C8, STUB_SPY_TARGET), (0xD8, STUB_SPY_UNLIMBO),
                       (0x1EC, STUB_SPY_COMMENCE), (0x20, STUB_SPY_DELETE)):
        emu.write32(SPY_PLANE_VT + slot, stub)
    created = list(created if created is not None else [True] * count)
    unlimbo = list(unlimbo if unlimbo is not None else [True] * count)
    requested = [list(cell) for cell in picks]
    picks = list(picks)
    planes = []

    def plane(e):
        return planes.index(e.uc.reg_read(UC_X86_REG_ECX))

    def create(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != SPY_TYPE or e.arg(0) != HOUSE:
            raise OracleError('CreateObject on another type or house')
        made = created.pop(0)
        e.events.append(['create', e.read_i32(SCENARIO_INIT), made])
        if not made:
            return 0
        this = SPY_PLANES + SPY_PLANE_STRIDE * len(planes)
        planes.append(this)
        e.write32(this, SPY_PLANE_VT)
        return this

    def pick(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != MAP:
            raise OracleError('PickCellOnEdge on another map')
        out = e.arg(0)
        e.events.append(['pick_cell_on_edge', i32(e.arg(1)), hex(e.arg(2)), hex(e.arg(3)),
                         e.arg(4), e.arg(5) & 0xFF, e.arg(6) & 0xFF,
                         [read8(e, this + 0x3D4) for this in planes]])
        e.uc.mem_write(out, struct.pack('<hh', *picks.pop(0)))
        return out

    def unlimbo_stub(e):
        e.events.append(['unlimbo', plane(e), read_coord(e, e.arg(0)), e.arg(1),
                         e.read_i32(SCENARIO_INIT)])
        return int(unlimbo.pop(0))

    emu.hook(STUB_SPY_CREATE, create, 4)
    emu.hook(0x4AA440, pick, 0x1C)
    emu.hook(STUB_SPY_QUEUE, lambda e: e.events.append(
        ['queue', plane(e), e.arg(0), e.arg(1)]), 8)
    emu.hook(STUB_SPY_DESTINATION, lambda e: e.events.append(
        ['destination', plane(e), spy_name(e.arg(0)), e.arg(1) & 0xFF]), 8)
    emu.hook(STUB_SPY_TARGET, lambda e: e.events.append(
        ['target', plane(e), spy_name(e.arg(0))]), 4)
    emu.hook(STUB_SPY_UNLIMBO, unlimbo_stub, 8)
    emu.hook(STUB_SPY_COMMENCE, lambda e: e.events.append(['commence', plane(e)]), 0)
    emu.hook(STUB_SPY_DELETE, lambda e: e.events.append(
        ['delete', plane(e), e.arg(0) & 0xFF]), 4)
    sent = emu.invoke(0x65EAB0, ecx=HOUSE, edx=SPY_TYPE_INDEX,
                      args=[count, 0x1E, SPY_TARGET, 0])
    return dict(count=count, edge=edge, waypoint_edge=waypoint_edge,
                picks=requested, returned=i32(sent),
                mission_only=[read8(emu, this + 0x3D4) for this in planes],
                scenario_init=emu.read_i32(SCENARIO_INIT), events=emu.events)


def send_spy_planes():
    rows = [send_spy_planes_row(waypoint_edge=edge) for edge in (0, 1, 2, 3, -1, 4)]
    rows += [send_spy_planes_row(edge=2), send_spy_planes_row(edge=4, waypoint_edge=3),
             send_spy_planes_row(created=[False]), send_spy_planes_row(unlimbo=[False]),
             send_spy_planes_row(picks=((70, 45),))]
    return rows


def spyplane_mission_row(*, mission, target=True, nav_com=True, distance=0, weapon_range=5120,
                         damage=6, waypoint_edge=0, pick=(30, 1), in_playfield=True,
                         passive=False, latched=False, camera=17, frames=12):
    """Mission_SpyplaneApproach 0x4155F0 or Mission_SpyplaneOverfly 0x4157C0
    on a fixture plane at SPY_LOCATION, ReReveal 0x70B1D0 and UpdateReveal
    0x70AF50 run natively (Sight=0, no veterancy, Location 1500 leptons up)."""
    emu = Emu()
    plane = SPY_PLANES
    emu.write32(plane, SPY_PLANE_VT)
    emu.write32(plane + 0x21C, HOUSE)
    emu.write32(HOUSE + 0x34, HOUSE_TYPE)
    write8(emu, HOUSE_TYPE + 0x1A6, passive)
    emu.write32(HOUSE + 0x577C, waypoint_edge)
    emu.write32(plane + 0x2B4, SPY_TARGET if target else 0)
    emu.write32(plane + 0x5A4, SPY_NAV_COM if nav_com else 0)
    write_coord(emu, plane + 0x9C, SPY_LOCATION)
    write8(emu, plane + 0x3D5, in_playfield)
    write8(emu, plane + 0x250, latched)
    write_coord(emu, plane + 0x254, (30 * 256 + 128, 40 * 256 + 128, 1500))
    emu.write32(plane + 0x260, 4)
    emu.write32(SPY_TYPE + 0x5E8, 0)
    emu.write32(RULES + 0x16BC, 2000)
    emu.write32(RULES + 0x280, camera)
    emu.write32(RULES + 0x290, frames)
    emu.write32(SPY_WEAPON, SPY_WEAPON_TYPE)
    emu.write32(SPY_WEAPON_TYPE + 0xB4, weapon_range)
    emu.write32(SPY_WEAPON_TYPE + 0xA4, damage)
    for slot, stub in ((0x84, STUB_SPY_TECHNO_TYPE), (0x3F8, STUB_SPY_WEAPON),
                       (0x480, STUB_SPY_DESTINATION), (0x1E8, STUB_SPY_QUEUE),
                       (0x48C, 0x70B1D0), (0x488, 0x70AF50)):
        emu.write32(SPY_PLANE_VT + slot, stub)

    def distance_to(e):
        e.events.append(['distance_to', spy_name(e.arg(0))])
        return distance if e.arg(0) else 0

    def weapon(e):
        if e.arg(0) != 0:
            raise OracleError('GetWeapon for a secondary weapon')
        return SPY_WEAPON

    def pick_stub(e):
        e.events.append(['pick_cell_on_edge', i32(e.arg(1)), hex(e.arg(2)), hex(e.arg(3)),
                         e.arg(4), e.arg(5) & 0xFF, e.arg(6) & 0xFF])
        e.uc.mem_write(e.arg(0), struct.pack('<hh', *pick))
        return e.arg(0)

    def lookup(e):
        e.events.append(['map_cell', read_cell(e, e.arg(0))])
        return SPY_DESTINATION_CELL

    def reveal(e):
        e.events.append(['reveal', read_coord(e, e.arg(0)), i32(e.arg(1)),
                         spy_name(e.arg(2)) if e.arg(2) != HOUSE else 'owner',
                         e.arg(3) & 0xFF, e.arg(4) & 0xFF, e.arg(5) & 0xFF,
                         e.arg(6) & 0xFF, e.arg(7) & 0xFF])

    def fog_border(e):
        e.events.append(['fog_border', read_coord(e, e.arg(0)), e.arg(1) & 0xFF,
                         i32(e.arg(2)), e.arg(3) & 0xFF])

    def play_at(e):
        e.events.append(['play_at', i32(e.uc.reg_read(UC_X86_REG_ECX)),
                         read_coord(e, e.uc.reg_read(UC_X86_REG_EDX)), e.arg(0)])

    emu.hook(STUB_SPY_TECHNO_TYPE, lambda _e: SPY_TYPE, 0)
    emu.hook(STUB_SPY_WEAPON, weapon, 4)
    emu.hook(STUB_SPY_DESTINATION, lambda e: e.events.append(
        ['destination', spy_name(e.arg(0)), e.arg(1) & 0xFF]), 8)
    emu.hook(STUB_SPY_QUEUE, lambda e: e.events.append(['queue', e.arg(0), e.arg(1)]), 8)
    emu.hook(0x5F6440, distance_to, 4)
    emu.hook(0x4AA440, pick_stub, 0x1C)
    emu.hook(0x5657A0, lookup, 4)
    emu.hook(0x5678E0, reveal, 0x20)
    emu.hook(0x567DA0, fog_border, 0x10)
    emu.hook(0x7509E0, play_at, 4)
    entry = {'approach': 0x4155F0, 'overfly': 0x4157C0}[mission]
    frames_out = emu.invoke(entry, ecx=plane)
    return dict(mission=mission, target=target, nav_com=nav_com, distance=distance,
                weapon_range=weapon_range, damage=damage, waypoint_edge=waypoint_edge,
                pick=list(pick), in_playfield=in_playfield, passive=passive,
                latched=latched, camera=camera, frames=frames,
                returned=i32(frames_out), events=emu.events,
                action_latch=read8(emu, plane + 0x6D2),
                reveal_latch=read8(emu, plane + 0x250),
                reveal_radius=emu.read_i32(plane + 0x260),
                reveal_coord=read_coord(emu, plane + 0x254))


def spyplane_missions():
    rows = []
    for mission in ('approach', 'overfly'):
        def row(**kwargs):
            rows.append(spyplane_mission_row(mission=mission, **kwargs))
        for distance in (0, 1, 0x2FF, 0x300, 0x301, 5119, 5120, 5121, 9000):
            row(distance=distance)
        for distance in (0x200, 4000, 9000):
            row(distance=distance, nav_com=False)
        row(target=False)
        row(target=False, nav_com=False)
        for waypoint_edge in (1, 2, 3, -1, 4):
            row(distance=0x200, nav_com=False, waypoint_edge=waypoint_edge)
        row(distance=0x200, nav_com=False, pick=(0, 0))
        row(distance=0x200, nav_com=False, pick=(0, 7))
        row(distance=100, passive=True)
        row(distance=100, in_playfield=False)
        row(distance=100, latched=True)
        row(distance=100, camera=-1)
        row(distance=100, damage=0)
        row(distance=100, damage=11)
        row(distance=100, weapon_range=0)
        row(distance=100, frames=0)
    return rows


def aircraft_leave_map_row(*, cell, fly_by=False, fly_back=False, target=False,
                           current=0x1F, queued=-1, in_playfield=True, team=None,
                           mission_only=True):
    """AircraftClass::AI's removal block 0x414F47..0x414FDF run as a slice on
    a fixture frame (ESI the plane), with GetMapCoords 0x41BEA0, In_Bounds
    0x568300, IsCellInPlayfield 0x578460 (mode one; every cell lookup misses),
    the predicate 0x41B890 and Get_Mission 0x5B3040 run natively."""
    emu = Emu()
    install_playfield(emu)
    emu.write32(MAP + 0xF8, SPY_SIZE_HEIGHT)
    plane = SPY_PLANES
    emu.write32(plane, SPY_PLANE_VT)
    emu.write32(plane + 0x6C4, SPY_TYPE)
    write8(emu, SPY_TYPE + 0xE0B, fly_by)
    write8(emu, SPY_TYPE + 0xE0C, fly_back)
    write_coord(emu, plane + 0x9C, (cell[0] * 256 + 128, cell[1] * 256 + 128, 1500))
    emu.write32(plane + 0x2B4, SPY_TARGET if target else 0)
    emu.write32(plane + 0xAC, current)
    emu.write32(plane + 0xB4, queued)
    write8(emu, plane + 0x3D5, in_playfield)
    write8(emu, plane + 0x3D4, mission_only)
    emu.write32(plane + 0x5D4, SPY_TEAM if team is not None else 0)
    for slot, stub in ((0x1B8, 0x41BEA0), (0x4DC, 0x41B890), (0x184, 0x5B3040),
                       (0xF8, STUB_SPY_UNINIT)):
        emu.write32(SPY_PLANE_VT + slot, stub)

    def team_stub(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != SPY_TEAM:
            raise OracleError('the team call on another object')
        e.events.append(['team'])
        return int(team)

    emu.hook(0x6EC300, team_stub, 0)
    emu.hook(STUB_SPY_UNINIT, lambda e: e.events.append(['uninit']), 0)
    emu.mark(0x568300, ['in_bounds'])
    emu.mark(0x578460, ['in_playfield'])
    uc = emu.uc
    sp = STACK_BASE + STACK_SIZE - 0x1000
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_ESI, plane)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    end = run_checked(uc, 0x414F47, (0x414F99, 0x414FD7, 0x414FDF), count=100_000)
    return dict(cell=list(cell), fly_by=fly_by, fly_back=fly_back, target=target,
                current=current, queued=queued, in_playfield=in_playfield, team=team,
                mission_only=mission_only, playfield=PLAYFIELD,
                size_height=SPY_SIZE_HEIGHT, removed=end != 0x414FDF, events=emu.events)


def aircraft_leave_map():
    rows = []
    # Inside the playfield; outside it but in the Size diamond (low sum, a
    # wide difference); outside the diamond on each of its four sides.
    cells = ((30, 30), (22, 22), (60, 30), (19, 19), (90, 45), (61, 20), (20, 61))
    for cell in cells:
        for fly_by, fly_back in ((False, False), (True, False), (False, True)):
            rows.append(aircraft_leave_map_row(cell=cell, fly_by=fly_by, fly_back=fly_back))
    for cell in ((22, 22), (19, 19)):
        for case in (dict(target=True), dict(target=True, current=0x1E),
                     dict(in_playfield=False), dict(current=0x1A), dict(current=0x1B),
                     dict(current=-1, queued=0x1A), dict(current=-1, queued=0x1B),
                     dict(current=-1, queued=-1), dict(current=4),
                     dict(mission_only=False), dict(team=False), dict(team=True),
                     dict(team=False, mission_only=False)):
            rows.append(aircraft_leave_map_row(cell=cell, **case))
    return rows


# ---------------------------------------------------------------- team actions

TEAM_ACTION_ENTRIES = {55: 0x6EFC70, 57: 0x6F0130}
TEAM_SW = BASE + 0x3C0000
TEAM_SW_TEAM = TEAM_SW
TEAM_SW_TEAM_TYPE = TEAM_SW + 0x1000
TEAM_SW_CENTRE = TEAM_SW + 0x2000
TEAM_SW_TARGET = TEAM_SW + 0x2100
TEAM_SW_NODE = TEAM_SW + 0x2200
TEAM_SW_VT = TEAM_SW + 0x3000
TEAM_SW_TYPES = TEAM_SW + 0x8000
TEAM_SW_MEMBERS = TEAM_SW + 0x20000
TEAM_SW_STRIDE = 0x1000
# The fixture members' Location: the coordinate Greatest_Threat searches from.
# The rows' cells fit the Rust replay's 32-cell arena.
TEAM_SW_LOCATION = (10 * 256 + 100, 11 * 256 + 20, 208)
TEAM_SW_CENTRE_COORDS = (12 * 256 + 128, 13 * 256 + 128, 0)
TEAM_SW_TARGET_COORDS = (20 * 256 + 128, 21 * 256 + 128, 0)

STUB_TEAM_SW_TYPE = STUBS + 0x800
STUB_TEAM_SW_WHAT = STUBS + 0x810
STUB_TEAM_SW_COORDS = STUBS + 0x820
STUB_TEAM_SW_THREAT = STUBS + 0x830


def team_sw(kind, *, charged=False, granted=True, start=-1, left=0, custom=-1, recharge=900):
    """A Super of `Type=` `kind`: `+0x6F`, `+0x6D`, its RechargeTimer (`+0x30`
    start, `+0x38` time left), `+0x24` (CustomChargeTime) and its type's
    RechargeTime (`+0xB0`, frames)."""
    return dict(kind=kind, charged=charged, granted=granted, start=start, left=left,
                custom=custom, recharge=recharge)


def team_member(*, rating=0, live=True, joined=True, aircraft=False):
    return dict(rating=rating, live=live, joined=joined, aircraft=aircraft)


def team_super_row(*, action, supers, members=(team_member(),), output=100, drain=50,
                   percent=0.7, frame=5000, centre=TEAM_SW_CENTRE_COORDS, argument=9,
                   only_enemy=False, target=None):
    """TeamClass::AI's script action 55 (0x6EFC70) or 57 (0x6F0130), called as
    the jump table calls it (0x6E9D95, 0x6E9DC7: the {action, argument} node and
    the first-frame flag): the leader loop with the live test 0x6EF9E0, the
    Supers search, GetPowerRatio 0x4FCE30, the RechargeTimer read,
    GetRechargeTime 0x6CC260 and Quarry_To_Threat 0x645BB0 run natively; the
    house is the computer house holding the row's Supers."""
    emu = Emu()
    install_supers(emu, [(sw['kind'], sw['charged']) for sw in supers])
    for index, sw in enumerate(supers):
        this = AI_SUPERS + index * 0x100
        write8(emu, this + 0x6D, sw['granted'])
        emu.write32(this + 0x24, sw['custom'])
        emu.write32(this + 0x30, sw['start'])
        emu.write32(this + 0x38, sw['left'])
        emu.write32(AI_SW_TYPES + index * 0x100 + 0xB0, sw['recharge'])
    emu.write32(HOUSE + 0x53A4, output)
    emu.write32(HOUSE + 0x53A8, drain)
    emu.write32(RULES + 0xD70, f32_bits(percent))
    emu.write32(FRAME, frame)
    emu.write32(SCENARIO_INIT, 0)

    team = TEAM_SW_TEAM
    emu.write32(team + 0x24, TEAM_SW_TEAM_TYPE)
    write8(emu, TEAM_SW_TEAM_TYPE + 0xF7, only_enemy)
    emu.write32(team + 0x34, TEAM_SW_CENTRE)
    write8(emu, team + 0x80, 0)
    emu.write32(TEAM_SW_CENTRE, TEAM_SW_VT)
    emu.write32(TEAM_SW_TARGET, TEAM_SW_VT)
    emu.write32(TEAM_SW_NODE, action)
    emu.write32(TEAM_SW_NODE + 4, argument)
    pointers = [TEAM_SW_MEMBERS + index * TEAM_SW_STRIDE for index in range(len(members))]
    emu.write32(team + 0x54, pointers[0] if pointers else 0)
    for index, (this, member) in enumerate(zip(pointers, members)):
        emu.write32(this, TEAM_SW_VT)
        emu.write32(this + 0x5D8, pointers[index + 1] if index + 1 < len(pointers) else 0)
        emu.write32(TEAM_SW_TYPES + index * TEAM_SW_STRIDE + 0x5FC, member['rating'])
        write8(emu, this + 0x90, member['live'])
        emu.write32(this + 0x6C, 100)
        write8(emu, this + 0x81, 0)
        write8(emu, this + 0x689, member['joined'])
        emu.write32(this + 0x21C, HOUSE)
        write_coord(emu, this + 0x9C, TEAM_SW_LOCATION)
    for slot, stub in ((0x84, STUB_TEAM_SW_TYPE), (0x2C, STUB_TEAM_SW_WHAT),
                       (0x48, STUB_TEAM_SW_COORDS), (0x3C4, STUB_TEAM_SW_THREAT)):
        emu.write32(TEAM_SW_VT + slot, stub)

    def member_index(e):
        this = e.uc.reg_read(UC_X86_REG_ECX)
        if this not in pointers:
            raise OracleError(f'a member call on {this:#x}')
        return pointers.index(this)

    def coords(e):
        this = e.uc.reg_read(UC_X86_REG_ECX)
        answer = {TEAM_SW_CENTRE: centre, TEAM_SW_TARGET: target}.get(this)
        if answer is None:
            raise OracleError(f'GetCoords on {this:#x}')
        write_coord(e, e.arg(0), answer)
        return e.arg(0)

    def threat(e):
        e.events.append(['threat', member_index(e), i32(e.arg(0)), read_coord(e, e.arg(1)),
                         e.arg(2) & 0xFF])
        return TEAM_SW_TARGET if target is not None else 0

    def assign(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != team:
            raise OracleError('Assign_Mission_Target on another team')
        if e.arg(0) != TEAM_SW_TARGET:
            raise OracleError('Assign_Mission_Target of another target')
        e.events.append(['assign'])

    emu.hook(STUB_TEAM_SW_TYPE,
             lambda e: TEAM_SW_TYPES + member_index(e) * TEAM_SW_STRIDE, 0)
    emu.hook(STUB_TEAM_SW_WHAT,
             lambda e: WHAT['aircraft' if members[member_index(e)]['aircraft'] else 'unit'], 0)
    emu.hook(STUB_TEAM_SW_COORDS, coords, 4)
    emu.hook(STUB_TEAM_SW_THREAT, threat, 0xC)
    emu.hook(0x6E9050, assign, 4)
    record_fire(emu)
    emu.invoke(TEAM_ACTION_ENTRIES[action], ecx=team, args=[TEAM_SW_NODE, 0])
    return dict(action=action, argument=argument, supers=list(supers), members=list(members),
                output=output, drain=drain, percent=f32_bits(percent), frame=frame,
                centre=list(centre), only_enemy=only_enemy,
                target=None if target is None else list(target),
                location=list(TEAM_SW_LOCATION), events=emu.events,
                complete=read8(emu, team + 0x80))


def team_super_actions():
    rows = []
    frame = 5000

    def charging(remaining, recharge=900):
        """A granted Super `remaining` frames from charged."""
        return dict(start=frame - (recharge - remaining), left=recharge, recharge=recharge)

    expired = dict(start=frame - 900, left=900)
    for action, own in ((55, 1), (57, 3)):
        def retail(sw):
            """Retail's [SuperWeaponTypes] kinds 0..4 with the action's own
            Super at its own index, so its Type= value fires itself."""
            supers = [team_sw(kind) for kind in range(5)]
            supers[own] = sw
            return supers

        def row(supers, **kwargs):
            rows.append(team_super_row(action=action, supers=supers, frame=frame, **kwargs))

        # Charged with full power: fire (57 asks for a target first; NULL
        # here). Power ratios at and around one.
        for output, drain in ((100, 50), (50, 50), (0, 0), (5, 0), (0, 10), (49, 50),
                              (-10, -5), (-5, -10), (1, 2)):
            row(retail(team_sw(own, charged=True, **expired)), output=output, drain=drain)
        # Not charged: wait while the charge is nearly full, else move on.
        for remaining in (0, 1, 269, 270, 271, 300, 899, 900):
            row(retail(team_sw(own, **charging(remaining))))
        for remaining in (0, 270, 271):
            row(retail(team_sw(own, granted=False, **charging(remaining))))
        for left in (0, 200, 270, 271, 500, -5):
            row(retail(team_sw(own, start=-1, left=left)))
        row(retail(team_sw(own, start=frame - 2000, left=900)))
        row(retail(team_sw(own, start=frame + 10, left=900)))
        for start, left, recharge in ((-1, 0, 0), (-1, 5, 0), (-1, -5, 0), (frame, 5, -900)):
            row(retail(team_sw(own, start=start, left=left, recharge=recharge)))
        for percent in (0.0, 0.5, 1.0, 0.25, -0.5, 1.5):
            for remaining in (0, 225, 226, 450, 451, 900):
                row(retail(team_sw(own, **charging(remaining))), percent=percent)
        # A per-Super CustomChargeTime (+0x24) replaces the type's.
        row(retail(team_sw(own, custom=1000, **charging(280))))
        row(retail(team_sw(own, custom=800, **charging(250))))
        # Charged without full power: the wait test on an expired timer.
        row(retail(team_sw(own, charged=True, **expired)), output=10, drain=50)
        row(retail(team_sw(own, charged=True, granted=False, **expired)), output=10, drain=50)
        # No members: done at once.
        row(retail(team_sw(own, charged=True, **expired)), members=())
        # The centre's cell, rounding toward zero.
        for centre in ((0, 0, 0), (255, 256, 0), (-1, -255, 0), (-256, -257, 0),
                       (-513, 513, 0), (12 * 256 + 255, 13 * 256, 999)):
            row(retail(team_sw(own, charged=True, **expired)), centre=centre)

    # Which Super: action 55 checks the first of Type= 1, 57 the last of
    # Type= 3 and of Type= 4; each fires the index its Super's Type= value
    # names.
    def iron(supers, **kwargs):
        rows.append(team_super_row(action=55, supers=supers, frame=frame, **kwargs))

    def chrono(supers=None, **kwargs):
        if supers is None:
            supers = [team_sw(0), team_sw(1), team_sw(2), team_sw(3, charged=True, **expired),
                      team_sw(4)]
        rows.append(team_super_row(action=57, supers=supers, frame=frame, **kwargs))

    target = TEAM_SW_TARGET_COORDS
    iron([team_sw(0), team_sw(3, charged=True, **expired), team_sw(2), team_sw(3), team_sw(4)])
    iron([team_sw(0), team_sw(1, **charging(800)), team_sw(1, charged=True, **expired),
          team_sw(3), team_sw(4)])
    iron([team_sw(0), team_sw(1, charged=True, **expired), team_sw(1, **charging(800)),
          team_sw(3), team_sw(4)])
    iron([team_sw(1, charged=True, **expired), team_sw(5), team_sw(2), team_sw(3),
          team_sw(4)])
    chrono([team_sw(0), team_sw(1), team_sw(3, charged=True, **expired),
            team_sw(3, **charging(800)), team_sw(4)], target=target)
    chrono([team_sw(0), team_sw(1), team_sw(3, **charging(800)),
            team_sw(3, charged=True, **expired), team_sw(4)], target=target)
    chrono([team_sw(0), team_sw(1), team_sw(2), team_sw(1), team_sw(4)], target=target)
    chrono([team_sw(0), team_sw(1), team_sw(2), team_sw(3, charged=True, **expired),
            team_sw(0)], target=target)
    chrono([team_sw(4), team_sw(1), team_sw(2), team_sw(3, charged=True, **expired),
            team_sw(0)], target=target)
    chrono([team_sw(0), team_sw(1), team_sw(2), team_sw(3, charged=True, **expired),
            team_sw(4, granted=False)], target=target)
    # Action 57 with a target: both fires, then the mission target.
    chrono(target=target)
    chrono(target=target, only_enemy=True)
    for argument in (0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, -1):
        chrono(target=target, argument=argument)
    for coords in ((0, 0, 0), (-1, -257, 0), (255, 256, 0), (20 * 256 + 255, 21 * 256, 77)):
        chrono(target=coords)
    chrono(target=target, output=10, drain=50)
    # The leader: the highest LeadershipRating= among live members that have
    # joined (+0x689) or are aircraft, the first on a tie, else the head.
    for members in (
            (team_member(rating=9, live=False), team_member(rating=3),
             team_member(rating=7, joined=False),
             team_member(rating=5, joined=False, aircraft=True)),
            (team_member(rating=4), team_member(rating=4)),
            (team_member(rating=4, joined=False), team_member(rating=2, live=False)),
            (team_member(rating=-5), team_member(rating=0)),
            (team_member(rating=-5), team_member(rating=-1)),
            (team_member(rating=1, live=False, aircraft=True), team_member(rating=0))):
        chrono(target=target, members=members)
    return rows


IRON_TINT = BASE + 0x3F0000
IRON_TINT_TECHNO = IRON_TINT
IRON_TINT_VT = IRON_TINT + 0x1000
# TechnoClass: IronCurtainTimer (+0x18C start, +0x194 time left), IronTintTimer
# (+0x198, +0x1A0), IronTintStage, and the Force Shield byte IronCurtain writes.
TECHNO_IC_TIMER, TECHNO_TINT_TIMER = 0x18C, 0x198
TECHNO_TINT_STAGE, TECHNO_FORCE_SHIELDED = 0x1A4, 0x1C4


def iron_tint_row(*, duration, frames, applies=(0,), force_shield=False, seed=7, start=1000,
                  before=2):
    """A constructed Techno (stage 10, both timers stopped at 0) whose
    UpdateIronTint 0x70E5A0 runs once a frame from `before` frames ahead of
    offset 0 to offset `frames` - 1, with TechnoClass::IronCurtain 0x70E2B0
    (house 0) ahead of it at each offset in `applies`. IsIronCurtained
    0x41BF40 (vt+0x160), CDTimerClass::Remaining 0x4B4D70 and the Scenario
    RandomRanged 0x65C7E0 (seeded by 0x65C6D0) run natively. A step is a
    frame whose stage or tint timer changed or that drew: [offset, stage,
    tint timer start, tint timer time left, draws]."""
    emu = Emu()
    this = IRON_TINT_TECHNO
    emu.write32(this, IRON_TINT_VT)
    emu.write32(IRON_TINT_VT + 0x160, 0x41BF40)
    for timer in (TECHNO_IC_TIMER, TECHNO_TINT_TIMER):
        emu.write32(this + timer, 0xFFFFFFFF)
        emu.write32(this + timer + 8, 0)
    emu.write32(this + TECHNO_TINT_STAGE, 10)
    seed_scenario_rng(emu, seed)
    record_draws(emu)

    def state():
        return [emu.read_i32(this + TECHNO_TINT_STAGE), emu.read_i32(this + TECHNO_TINT_TIMER),
                emu.read_i32(this + TECHNO_TINT_TIMER + 8)]

    steps, last = [], state()
    for offset in range(-before, frames):
        emu.write32(FRAME, (start + offset) & 0xFFFFFFFF)
        emu.events = []
        if offset in applies:
            emu.invoke(0x70E2B0, ecx=this, args=[duration, 0, int(force_shield)])
        emu.invoke(0x70E5A0, ecx=this)
        now = state()
        if now != last or emu.events:
            steps.append([offset, *now, [event[2:] for event in emu.events]])
        last = now
    return dict(duration=duration, frames=frames, applies=list(applies),
                force_shield=force_shield, seed=seed, start=start, before=before, steps=steps,
                force_shielded=emu.read_i32(this + TECHNO_FORCE_SHIELDED),
                curtain_timer=[emu.read_i32(this + TECHNO_IC_TIMER),
                               emu.read_i32(this + TECHNO_IC_TIMER + 8)],
                rng_after=rng_state(emu))


def iron_tint():
    """The retail Iron Curtain and Force Shield durations to past their end,
    a re-application before the draw and one after it, and short durations
    around each test of the curtain's time left (54 at stage 5, 30 at 6).
    Seed 7 draws 4 first (stage 3 lasts 24 frames), so its stage 5 first runs
    out at offset 58: curtains of 111, 112 and 113 frames have 53, 54 and 55
    left there."""
    rows = [iron_tint_row(duration=750, frames=760),
            iron_tint_row(duration=500, frames=510, force_shield=True, seed=11),
            iron_tint_row(duration=750, frames=120, applies=(0, 9, 40), seed=3)]
    for duration in (1, 6, 10, 11, 30, 31, 40, 54, 55, 60, 84, 85, 100):
        rows.append(iron_tint_row(duration=duration, frames=duration + 3, seed=duration))
    for duration in (111, 112, 113):
        rows.append(iron_tint_row(duration=duration, frames=duration + 3, seed=7))
    return rows


CURTAIN_TECHNO = BASE + 0x3F2000
CURTAIN_VT = BASE + 0x3F3000
# TechnoClass: the flash count +0xF0, AirstrikeTintTimer (+0x1B4 start,
# +0x1BC time left), AirstrikeTintStage, Deactivated and Berzerk.
TECHNO_FLASH, TECHNO_AIRSTRIKE_TIMER, TECHNO_AIRSTRIKE_STAGE = 0xF0, 0x1B4, 0x1C0
TECHNO_DEACTIVATED, TECHNO_BERZERK = 0x1C8, 0x298
# RulesClass: [ColorAdd] (sixteen RGB triples) and the [AudioVisual] indexes
# IronCurtainColor= and BerserkColor= read at 0x66B838 and 0x66B857.
RULES_COLOR_ADD, RULES_IRON_CURTAIN_COLOR, RULES_BERSERK_COLOR = 0x1874, 0x18A8, 0x18AC
# The surface pixel format 0x4BBC90 returns; 2 is RGB565, the active retail one
# (docs/research/LIGHTCONVERT_ROW_RGB565_ORACLE_2026_09_09.md).
PIXEL_FORMAT = 0x8205D0
# Retail RULESMD.INI [ColorAdd], in order.
RETAIL_COLOR_ADD = ((0, 0, 0), (31, 0, 0), (0, 63, 0), (0, 0, 31), (24, 0, 0), (0, 56, 0),
                    (0, 0, 24), (31, 63, 31), (7, 7, 7), (24, 56, 24), (14, 28, 14), (15, 0, 15),
                    (24, 56, 0), (16, 32, 0))
# The draw intensities each tint stage is applied to: the 0..2000 scale, its
# cap, and values the 32-bit product wraps on.
TINT_INTENSITIES = (0, 1, 199, 255, 256, 500, 999, 1000, 1001, 1234, 1500, 1999, 2000, 2001,
                    4000, -1, -256, -1000, 0x400000, 0x7FFFFFFF, -0x80000000)
# Curtain timers (start, duration) at frame 5000: time left 1 and 0, a stopped
# timer holding time and one holding none, and one starting after the frame.
CURTAIN_EDGES = ((4990, 11), (4990, 10), (-1, 7), (-1, 0), (5001, 750))
# Each tint stage with a tint timer (start, duration) it can hold at frame 5000.
CURTAIN_STAGE_TINTS = ((0, (-1, 0)), (1, (4998, 6)), (2, (4998, 4)), (3, (4980, 24)),
                       (4, (4996, 8)), (5, (4990, 16)), (6, (4996, 8)), (7, (4999, 6)),
                       (8, (4997, 4)), (9, (4990, 20)), (10, (4990, 20)))


def curtain_techno(emu, *, stage=0, tint=(-1, 0), curtain=(-1, 0)):
    """A fixture Techno: IronTintStage, IronTintTimer (start, time left) and
    IronCurtainTimer as given; the airstrike tint at its constructor's stage 0
    with both timers stopped."""
    this = CURTAIN_TECHNO
    emu.write32(this + TECHNO_IC_TIMER, curtain[0])
    emu.write32(this + TECHNO_IC_TIMER + 8, curtain[1])
    emu.write32(this + TECHNO_TINT_TIMER, tint[0])
    emu.write32(this + TECHNO_TINT_TIMER + 8, tint[1])
    emu.write32(this + TECHNO_TINT_STAGE, stage)
    emu.write32(this + TECHNO_AIRSTRIKE_TIMER, 0xFFFFFFFF)
    emu.write32(this + TECHNO_AIRSTRIKE_TIMER + 8, 0)
    emu.write32(this + TECHNO_AIRSTRIKE_STAGE, 0)
    return this


def effect_tint_intensity():
    """TechnoClass::GetEffectTintIntensity 0x70E360 (ScaleByIronTintPhase
    0x70E380, then the airstrike phase 0x70E4B0 at stage 0) over the domain
    UpdateIronTint writes (stages 0 to 10, iron_tint): every stage with each
    time left its timer can hold (0 to 25 frames, the longest being stage 3's
    20 plus 5), the stopped timer IronCurtain leaves (start -1, 0 left) and
    one past its end, each applied to TINT_INTENSITIES. The function only
    reads, so one emulator serves every row. A row is [stage, tint timer
    start, time left field, frame, results in TINT_INTENSITIES' order]."""
    emu = Emu()
    frame = 5000
    emu.write32(FRAME, frame)
    rows = []
    for stage in range(11):
        timers = [(-1, 0), (frame - 30, 25)]
        timers += [(frame - 3, left + 3) for left in range(26)]
        for start, field in timers:
            this = curtain_techno(emu, stage=stage, tint=(start, field))
            rows.append([stage, start, field, frame,
                         [i32(emu.invoke(0x70E360, ecx=this, args=[intensity & 0xFFFFFFFF]))
                          for intensity in TINT_INTENSITIES]])
    return dict(intensities=list(TINT_INTENSITIES), rows=rows)


def curtain_draw_arm_row(*, intensity=1000, word=0, curtain=(4990, 750), stage=2,
                         tint=(4998, 4), flash=0, berzerk=False, deactivated=False,
                         iron_color=0, berserk_color=4, pixel_format=2, frame=5000):
    """UnitClass::DrawVoxelBody's intensity and colour block 0x73BF7B..0x73C166
    as a slice: EBP the unit (vtable slots +0x464 and +0x160 the native
    flash arm 0x70D190 and IsIronCurtained 0x41BF40), [ESP+0x1E0] the draw
    intensity and [ESP+0x1E4] the colour word its caller passed. Rules hold
    the retail [ColorAdd] and the row's indexes; 0x4BBC90 reads the row's
    pixel format. The result is ECX (the intensity the composite blit takes)
    and ESI (its colour word) at 0x73C166."""
    emu = Emu()
    emu.write32(FRAME, frame)
    this = curtain_techno(emu, stage=stage, tint=tint, curtain=curtain)
    emu.write32(this, CURTAIN_VT)
    emu.write32(CURTAIN_VT + 0x464, 0x70D190)
    emu.write32(CURTAIN_VT + 0x160, 0x41BF40)
    emu.write32(this + TECHNO_FLASH, flash)
    write8(emu, this + TECHNO_DEACTIVATED, int(deactivated))
    write8(emu, this + TECHNO_BERZERK, int(berzerk))
    for index, rgb in enumerate(RETAIL_COLOR_ADD):
        emu.uc.mem_write(RULES + RULES_COLOR_ADD + 3 * index, bytes(rgb))
    emu.write32(RULES + RULES_IRON_CURTAIN_COLOR, iron_color)
    emu.write32(RULES + RULES_BERSERK_COLOR, berserk_color)
    emu.write32(PIXEL_FORMAT, pixel_format)
    uc = emu.uc
    sp = STACK_BASE + STACK_SIZE - 0x1000
    uc.mem_write(sp + 0x1E0, u32(intensity))
    uc.mem_write(sp + 0x1E4, u32(word))
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_EBP, this)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    run_checked(uc, 0x73BF7B, 0x73C166, count=10_000)
    return dict(intensity=intensity, word=word, curtain=list(curtain), stage=stage,
                tint=list(tint), flash=flash, berzerk=berzerk, deactivated=deactivated,
                iron_color=iron_color, berserk_color=berserk_color,
                pixel_format=pixel_format, frame=frame,
                out_intensity=i32(uc.reg_read(UC_X86_REG_ECX)),
                out_word=uc.reg_read(UC_X86_REG_ESI))


def curtain_draw_arm():
    """The arm's gate at the curtain's edges (time left 1 and 0, a stopped
    timer holding time and none), each stage's scale at the draw intensities
    of a lit and a dark cell, and the other arms: the flash count's bit 1,
    IronCurtainColor indexes other than retail's 0 in each pixel format,
    Berzerk, Deactivated and a caller's colour word."""
    rows = []
    for curtain in CURTAIN_EDGES:
        rows.append(curtain_draw_arm_row(curtain=curtain))
    for stage, tint in CURTAIN_STAGE_TINTS:
        for intensity in (1000, 1500, 300):
            rows.append(curtain_draw_arm_row(stage=stage, tint=tint, intensity=intensity))
    for flash in (2, 3, 1):
        rows.append(curtain_draw_arm_row(flash=flash, intensity=1000))
        rows.append(curtain_draw_arm_row(flash=flash, intensity=1600))
    for pixel_format in (2, 1, 0):
        rows.append(curtain_draw_arm_row(iron_color=4, pixel_format=pixel_format))
        rows.append(curtain_draw_arm_row(iron_color=9, pixel_format=pixel_format))
    rows.append(curtain_draw_arm_row(berzerk=True))
    rows.append(curtain_draw_arm_row(berzerk=True, curtain=(-1, 0)))
    rows.append(curtain_draw_arm_row(deactivated=True))
    rows.append(curtain_draw_arm_row(deactivated=True, intensity=-7))
    rows.append(curtain_draw_arm_row(word=0x1234))
    return rows


# ------------------------------------------------- the curtain's tint on buildings

TINT_FIXTURE = BASE + 0x3F4000
TINT_CELL = TINT_FIXTURE
TINT_OTHER = TINT_FIXTURE + 0x200
TINT_OTHER_VT = TINT_FIXTURE + 0x300
TINT_AIRSTRIKE = TINT_FIXTURE + 0x400
TINT_ANIM = TINT_FIXTURE + 0x800
TINT_ANIM_VT = TINT_FIXTURE + 0xA00
TINT_ANIM_TYPES = TINT_FIXTURE + 0x1000
TINT_SLOT_ANIMS = TINT_FIXTURE + 0x1800
TINT_CONVERT = TINT_FIXTURE + 0x2000
TINT_BLITTER = TINT_FIXTURE + 0x2400
TINT_ZBUFFER = TINT_FIXTURE + 0x2480
TINT_ABUFFER = TINT_FIXTURE + 0x24C0
TINT_LINES = TINT_FIXTURE + 0x2800
BLIT_TABLES, BLIT_TABLES_SIZE = 0x70000000, 0x80000
STUB_TINT_WHAT = STUBS + 0xA00
STUB_TINT_COORDS = STUBS + 0xA10
STUB_TINT_ANIM_COORDS = STUBS + 0xA20
STUB_TINT_OTHER_WHAT = STUBS + 0xA30
STUB_TINT_GET_CELL = STUBS + 0xA40
# TechnoClass: the AirstrikeClass aimed at it (+0x294), whose target is +0x50.
TECHNO_AIRSTRIKE = 0x294
# RulesClass: the [AudioVisual] indexes LaserTargetColor= and ForceShieldColor=
# read at 0x66B818 and 0x66B891.
RULES_LASER_TARGET_COLOR, RULES_FORCE_SHIELD_COLOR = 0x18A4, 0x18B0
# The byte CellClass 0x47C520 requires before it walks a cell's objects.
CELL_OBJECTS_LIVE = 0xA8E9A0
# The ZBuffer and ABuffer instances the blitters wrap their line pointers by.
ZBUFFER_PTR, ABUFFER_PTR = 0x887644, 0x87E8A4


def tint_building(emu, *, curtain=(4990, 750), stage=0, tint=(-1, 0), shielded=1,
                  airstrike=None, kind='building', flash=0,
                  coords=(10 * 256 + 128, 12 * 256 + 128, 0)):
    """curtain_techno's Techno as a building, a unit or an aircraft (`kind`):
    vt+0x160 the native IsIronCurtained 0x41BF40, vt+0x464 its class's flash
    arm (BuildingClass 0x456F80, TechnoClass 0x70D190), vt+0x2C WhatAmI
    answering the class's id and vt+0x48 GetCoords answering `coords`; the
    flash count, the Force Shield byte IronCurtain writes (+0x1C4), and an
    AirstrikeClass at +0x294 aimed at it ('self') or at another object
    ('other')."""
    this = curtain_techno(emu, stage=stage, tint=tint, curtain=curtain)
    emu.write32(this, CURTAIN_VT)
    emu.write32(CURTAIN_VT + 0x160, 0x41BF40)
    emu.write32(CURTAIN_VT + 0x464, 0x456F80 if kind == 'building' else 0x70D190)
    emu.write32(CURTAIN_VT + 0x2C, STUB_TINT_WHAT)
    emu.hook(STUB_TINT_WHAT, lambda _e: WHAT[kind], 0)
    emu.write32(CURTAIN_VT + 0x48, STUB_TINT_COORDS)
    emu.hook(STUB_TINT_COORDS, coords_stub(coords), 4)
    emu.write32(this + TECHNO_FLASH, flash)
    emu.write32(this + TECHNO_FORCE_SHIELDED, shielded)
    emu.write32(this + TECHNO_AIRSTRIKE, TINT_AIRSTRIKE if airstrike else 0)
    emu.write32(TINT_AIRSTRIKE + 0x50, this if airstrike == 'self' else TINT_OTHER)
    return this


def tint_rules(emu, *, force_color=6, laser_color=4, pixel_format=2):
    """Rules holding the retail [ColorAdd] and the row's LaserTargetColor= and
    ForceShieldColor= indexes; 0x4BBC90 answers the row's pixel format."""
    for index, rgb in enumerate(RETAIL_COLOR_ADD):
        emu.uc.mem_write(RULES + RULES_COLOR_ADD + 3 * index, bytes(rgb))
    emu.write32(RULES + RULES_LASER_TARGET_COLOR, laser_color)
    emu.write32(RULES + RULES_FORCE_SHIELD_COLOR, force_color)
    emu.write32(PIXEL_FORMAT, pixel_format)


def tint_shroud(emu, shrouded):
    """MapClass::operator[] 0x5657A0 answers the fixture cell, and 0x487950
    (whether the cell's centre is shrouded) the row's answer; both record."""
    def cell(e):
        e.events.append(['cell', list(struct.unpack('<hh', e.uc.mem_read(e.arg(0), 4)))])
        return TINT_CELL

    def shroud(e):
        e.events.append(['shroud', e.uc.reg_read(UC_X86_REG_ECX) == TINT_CELL])
        return int(shrouded)

    emu.hook(0x5657A0, cell, 4)
    emu.hook(0x487950, shroud, 0)


def drawshp_curtain_arm_row(*, kind='building', intensity=1000, curtain=(4990, 750), stage=2,
                            tint=(4998, 4), flash=0, airstrike=None, frame=5000):
    """TechnoClass::DrawSHP's intensity arm 0x70631F..0x706389 as a slice: ESI
    tint_building's Techno, [ESP+0x7C] the draw intensity. The result is EBP,
    the intensity DrawSHP hands the blit."""
    emu = Emu()
    emu.write32(FRAME, frame)
    this = tint_building(emu, curtain=curtain, stage=stage, tint=tint, kind=kind, flash=flash,
                         airstrike=airstrike)
    uc = emu.uc
    sp = STACK_BASE + STACK_SIZE - 0x1000
    uc.mem_write(sp + 0x7C, u32(intensity))
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_ESI, this)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    run_checked(uc, 0x70631F, 0x706389, count=10_000)
    return dict(kind=kind, intensity=intensity, curtain=list(curtain), stage=stage,
                tint=list(tint), flash=flash, airstrike=airstrike, frame=frame,
                out_intensity=i32(uc.reg_read(UC_X86_REG_EBP)))


def curtain_arm_rows(row, kinds):
    """For each class in `kinds`: the curtain's edges, each stage's scale at
    the intensities of a lit, a bright and a dark cell, the flash count's bit
    1 (each class's own arm), and an airstrike aimed at the object or
    elsewhere while the curtain is off and the stage still reads 2 (only a
    building's draw takes it)."""
    rows = []
    for kind in kinds:
        rows += [row(kind=kind, curtain=curtain) for curtain in CURTAIN_EDGES]
        for stage, tint in CURTAIN_STAGE_TINTS:
            rows += [row(kind=kind, stage=stage, tint=tint, intensity=intensity)
                     for intensity in (1000, 1500, 300)]
        for flash in (2, 3, 1):
            rows += [row(kind=kind, flash=flash, intensity=intensity)
                     for intensity in (1000, 1600)]
        rows += [row(kind=kind, curtain=(-1, 0), airstrike=airstrike)
                 for airstrike in ('self', 'other')]
    return rows


def drawshp_curtain_arm():
    """curtain_arm_rows for a building and a unit."""
    return curtain_arm_rows(drawshp_curtain_arm_row, ('building', 'unit'))


def draw_curtain_arm_row(*, kind='aircraft', intensity=1000, curtain=(4990, 750), stage=2,
                         tint=(4998, 4), flash=0, airstrike=None, frame=5000):
    """TechnoClass::Draw's intensity arm 0x706776..0x7067E4 as a slice: EBP
    tint_building's Techno, ECX the draw intensity (arg8, which 0x70676F
    loads; FootClass::Draw_A_VXL 0x4DAF10 passes on the one AircraftClass::
    Draw_It computes). The result is EDI, the intensity the voxel draw
    takes."""
    emu = Emu()
    emu.write32(FRAME, frame)
    this = tint_building(emu, curtain=curtain, stage=stage, tint=tint, kind=kind, flash=flash,
                         airstrike=airstrike)
    uc = emu.uc
    sp = STACK_BASE + STACK_SIZE - 0x1000
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_ECX, intensity & 0xFFFFFFFF)
    uc.reg_write(UC_X86_REG_EBP, this)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    run_checked(uc, 0x706776, 0x7067E4, count=10_000)
    if uc.reg_read(UC_X86_REG_ESP) != sp:
        raise OracleError('the arm left the stack unbalanced')
    return dict(kind=kind, intensity=intensity, curtain=list(curtain), stage=stage,
                tint=list(tint), flash=flash, airstrike=airstrike, frame=frame,
                out_intensity=i32(uc.reg_read(UC_X86_REG_EDI)))


def draw_curtain_arm():
    """curtain_arm_rows for an aircraft (AircraftClass::Draw_It's voxel) and a
    building (its voxel turret and barrel, BuildingClass::Draw 0x43E2FF)."""
    return curtain_arm_rows(draw_curtain_arm_row, ('aircraft', 'building'))


# ------------------------------------------------- the aircraft's light

# The aircraft translation unit's CRT initializers in their table's order
# (0x812404..0x81241C), up to the level step 0x889EC8 its draw divides by:
# the cell diagonal 0x889E70 and its ratios, the angles, then the step.
AIRCRAFT_TU_INITIALIZERS = (0x413AC0, 0x413AF0, 0x413B10, 0x413B30, 0x413B50, 0x413B70,
                            0x413B90)
AIRCRAFT_LEVEL_HEIGHT = 0x889EC8
AIRCRAFT_LIGHT = BASE + 0x100000
AIRCRAFT_LIGHT_VT = AIRCRAFT_LIGHT + 0x1000
AIRCRAFT_LIGHT_CELL = AIRCRAFT_LIGHT + 0x2000
AIRCRAFT_LIGHT_CELLS = AIRCRAFT_LIGHT + 0x10000
AIRCRAFT_LIGHT_CAPACITY = 0x10000
STUB_AIRCRAFT_HEIGHT = STUBS + 0xFA0
# MapClass's cell pointers (+0x13C) and their count (+0x140), which GetCellAt
# 0x565730 indexes by (Y / 256) * 512 + X / 256.
MAP_CELL_ITEMS, MAP_CELL_CAPACITY = 0x13C, 0x140
# CellClass's intensity word, ScenarioClass's Level= and IonLevel= (Set_Defaults
# 0x683901 and its Ion block) and RulesClass's ExtraAircraftLight=.
CELL_INTENSITY = 0x10A
SCENARIO_LEVEL, SCENARIO_ION_LEVEL = 0x3544, 0x355C
RULES_EXTRA_AIRCRAFT_LIGHT = 0x17DC


def aircraft_emu(fpcw):
    """An emulator after the aircraft unit's initializers under `fpcw`."""
    emu = Emu()
    for entry in AIRCRAFT_TU_INITIALIZERS:
        invoke_under(emu, entry, fpcw=fpcw)
    return emu


def aircraft_light_row(*, height=0, level=8, ion_level=0, storm=False, word=1000, extra=200,
                       coord=(40 * 256 + 128, 50 * 256 + 128, 900)):
    """AircraftClass::Draw_It's intensity 0x4148D1..0x41493E as a slice, after
    the unit's initializers under the startup word: EBP a fixture aircraft
    whose GetHeight vt+0x1C8 answers `height` and [ESP+0x28] the coordinate
    the draw built from its Location at 0x41468F. The storm byte IsActive
    0x53A100 reads, Scenario's Level= and IonLevel=, Rules'
    ExtraAircraftLight= and the intensity word of the one cell in Map's
    table are the row's; GetCellAt 0x565730 runs natively. The result is
    EBX, the intensity the draw hands FootClass::Draw_A_VXL."""
    emu = aircraft_emu(STARTUP_FPCW)
    this = AIRCRAFT_LIGHT
    emu.write32(this, AIRCRAFT_LIGHT_VT)
    emu.write32(AIRCRAFT_LIGHT_VT + 0x1C8, STUB_AIRCRAFT_HEIGHT)
    emu.hook(STUB_AIRCRAFT_HEIGHT, lambda _e: height, 0)
    write8(emu, STORM_ACTIVE, int(storm))
    emu.write32(SCENARIO + SCENARIO_LEVEL, level)
    emu.write32(SCENARIO + SCENARIO_ION_LEVEL, ion_level)
    emu.write32(RULES + RULES_EXTRA_AIRCRAFT_LIGHT, extra)
    emu.write32(MAP + MAP_CELL_ITEMS, AIRCRAFT_LIGHT_CELLS)
    emu.write32(MAP + MAP_CELL_CAPACITY, AIRCRAFT_LIGHT_CAPACITY)
    x, y, _ = coord
    emu.write32(AIRCRAFT_LIGHT_CELLS + 4 * ((y // 256) * 512 + x // 256), AIRCRAFT_LIGHT_CELL)
    emu.uc.mem_write(AIRCRAFT_LIGHT_CELL + CELL_INTENSITY, struct.pack('<h', word))
    uc = emu.uc
    sp = STACK_BASE + STACK_SIZE - 0x1000
    uc.mem_write(sp + 0x28, struct.pack('<iii', *coord))
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_EBP, this)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    run_checked(uc, 0x4148D1, 0x41493E, count=10_000)
    if uc.reg_read(UC_X86_REG_ESP) != sp:
        raise OracleError('the light block left the stack unbalanced')
    return dict(height=height, level=level, ion_level=ion_level, storm=storm, word=word,
                extra=extra, coord=list(coord), light=i32(uc.reg_read(UC_X86_REG_EBX)))


def aircraft_light():
    """The level step under the startup and the process control words, then
    the intensity over heights about each two-level step and below the
    ground, Level= and IonLevel= with and without a raging storm (a negative
    Level too), the cell's word at the signed 16-bit edges,
    ExtraAircraftLight= values (retail .2 is 200) and sums and products that
    wrap."""
    row = aircraft_light_row
    rows = [row(height=height, level=32)
            for height in (0, 1, 207, 208, 209, 415, 416, 1500, 2000, -1, -207, -208, -209,
                           -416)]
    for storm in (False, True):
        for height in (0, 416, 1500):
            rows.append(row(height=height, level=32, ion_level=20, storm=storm))
            rows.append(row(height=height, level=8, ion_level=-12, storm=storm))
    rows += [row(height=1500, word=word) for word in (0, 2000, -1, 0x7FFF, -0x8000)]
    rows += [row(height=1500, extra=extra) for extra in (0, -300, 0x7FFFFFFF)]
    rows.append(row(height=0x40000000, level=1000))
    rows.append(row(height=-0x80000000, level=3))
    return dict(level_height={f'0x{fpcw:04X}': aircraft_emu(fpcw).read_i32(AIRCRAFT_LEVEL_HEIGHT)
                              for fpcw in (STARTUP_FPCW, NATIVE_FPCW)},
                rows=rows)


def building_colour_word_row(*, curtain=(4990, 750), shielded=1, force_color=6, laser_color=4,
                             airstrike=None, shrouded=False, pixel_format=2, frame=5000):
    """The colour word a building's draws hand the blit, from both blocks that
    compute it, each as a slice on tint_building's building with tint_rules'
    Rules: BuildingClass_DrawBody's 0x43D386..0x43D544 (ESI the building, the
    word in EDI) and BuildingClass::Draw's 0x43DC1C..0x43DDF1 (EBP the
    building, the word in [ESP+0x1C]). The LaserTargetColor= colour while an
    airstrike is set, the ForceShieldColor= one while the building is
    curtained (IsIronCurtained 0x41BF40) with the Force Shield byte at 1, each
    converted by the pixel format 0x4BBC90 reads (2 is RGB565, the active
    retail one); none if 0x487950 calls the building's cell shrouded. Above
    the low 16 bits the conversion leaves stale register bits."""
    words, events = [], []
    for start, end, this_reg in ((0x43D386, 0x43D544, UC_X86_REG_ESI),
                                 (0x43DC1C, 0x43DDF1, UC_X86_REG_EBP)):
        emu = Emu()
        emu.write32(FRAME, frame)
        this = tint_building(emu, curtain=curtain, shielded=shielded, airstrike=airstrike)
        tint_rules(emu, force_color=force_color, laser_color=laser_color,
                   pixel_format=pixel_format)
        tint_shroud(emu, shrouded)
        uc = emu.uc
        sp = STACK_BASE + STACK_SIZE - 0x1000
        uc.reg_write(UC_X86_REG_ESP, sp)
        uc.reg_write(this_reg, this)
        uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        run_checked(uc, start, end, count=10_000)
        words.append(uc.reg_read(UC_X86_REG_EDI) if start == 0x43D386 else emu.read32(sp + 0x1C))
        events.append(emu.events)
    return dict(curtain=list(curtain), shielded=shielded, force_color=force_color,
                laser_color=laser_color, airstrike=airstrike, shrouded=shrouded,
                pixel_format=pixel_format, frame=frame, draw_body_word=words[0],
                draw_word=words[1], events=events)


def building_colour_word():
    """The Force Shield, the Iron Curtain (the byte at 0, and at 2), the
    curtain's edges, the shroud, an airstrike with and without the shield,
    every ForceShieldColor= index, and the other pixel formats."""
    row = building_colour_word_row
    rows = [row(), row(shielded=0), row(shielded=2), row(shrouded=True),
            row(airstrike='self', shielded=0), row(airstrike='self'),
            row(airstrike='self', shrouded=True), row(airstrike='other', shielded=0)]
    rows += [row(curtain=curtain) for curtain in CURTAIN_EDGES]
    rows += [row(force_color=index) for index in range(16)]
    for pixel_format in (1, 0, 3):
        rows += [row(pixel_format=pixel_format), row(pixel_format=pixel_format, force_color=9)]
    return rows


def anim_colour_word_row(*, slot_anim=True, occupant='building', curtain=(4990, 750),
                         shielded=1, force_color=6, airstrike=None, shrouded=False,
                         coords=(10 * 256 + 200, 12 * 256 + 40, 30), frame=5000):
    """AnimClass::DrawIt's colour block 0x4233EE..0x423630 as a slice: ESI a
    fixture anim (+0x118 the building-slot byte CreateAnimForSlot sets at
    0x45199B; vt+0x48 GetCoords answering `coords`). MapClass::operator[] by
    coordinate 0x565730 answers the fixture cell, whose ground objects (+0xE4,
    then each +0x30) 0x47C520 walks natively for the first building:
    `occupant` 'building' (the building alone), 'behind' (another object, then
    the building) or 'none' (the other object alone). The building is
    tint_building's and the Rules tint_rules'; 0x5657A0 and 0x487950 answer as
    in building_colour_word. The result is EBP at 0x423630 (also stored at
    [ESP+0x1C])."""
    emu = Emu()
    emu.write32(FRAME, frame)
    building = tint_building(emu, curtain=curtain, shielded=shielded, airstrike=airstrike)
    tint_rules(emu, force_color=force_color)
    tint_shroud(emu, shrouded)
    emu.write32(TINT_ANIM, TINT_ANIM_VT)
    emu.write32(TINT_ANIM_VT + 0x48, STUB_TINT_ANIM_COORDS)
    emu.hook(STUB_TINT_ANIM_COORDS, coords_stub(coords), 4)
    write8(emu, TINT_ANIM + 0x118, int(slot_anim))

    def cell_by_coord(e):
        e.events.append(['coord', read_coord(e, e.arg(0))])
        return TINT_CELL

    emu.hook(0x565730, cell_by_coord, 4)
    write8(emu, CELL_OBJECTS_LIVE, 1)
    emu.write32(TINT_OTHER, TINT_OTHER_VT)
    emu.write32(TINT_OTHER_VT + 0x2C, STUB_TINT_OTHER_WHAT)
    emu.hook(STUB_TINT_OTHER_WHAT, lambda _e: 1, 0)
    emu.write32(TINT_CELL + 0xE4, building if occupant == 'building' else TINT_OTHER)
    emu.write32(TINT_OTHER + 0x30, building if occupant == 'behind' else 0)
    emu.write32(building + 0x30, 0)
    uc = emu.uc
    sp = STACK_BASE + STACK_SIZE - 0x1000
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_ESI, TINT_ANIM)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    run_checked(uc, 0x4233EE, 0x423630, count=10_000)
    return dict(slot_anim=slot_anim, occupant=occupant, curtain=list(curtain),
                shielded=shielded, force_color=force_color, airstrike=airstrike,
                shrouded=shrouded, coords=list(coords), frame=frame,
                word=uc.reg_read(UC_X86_REG_EBP), stored=emu.read32(sp + 0x1C),
                events=emu.events)


def anim_colour_word():
    """The Force Shield's word on a slot anim; an anim off a slot; a cell whose
    first object is not the building, and one with no building; the Iron
    Curtain; the curtain's edges; the shroud; an airstrike; other
    ForceShieldColor= indexes; a coordinate west and north of the map."""
    row = anim_colour_word_row
    return [row(), row(slot_anim=False), row(occupant='behind'), row(occupant='none'),
            row(shielded=0), row(curtain=(4990, 11)), row(curtain=(4990, 10)),
            row(shrouded=True), row(airstrike='self', shielded=0), row(airstrike='self'),
            row(force_color=0), row(force_color=9), row(coords=(-3, -257, 0))]


def building_anim_light_row(*, intensity=1000, curtain=(4990, 750), stage=2, tint=(4998, 4),
                            flash=0, airstrike=None, frame=5000):
    """BuildingClass::UpdateAnimation's anim light 0x450A47..0x450A77 as a
    slice: EAX the Convert its vt+0x1E4 call left (the fixture Convert), ESI
    tint_building's building (vt+0x1BC GetCell answering the fixture cell,
    whose +0x10A holds `intensity`). 0x456FB0 (the flash arm, then
    GetEffectTintIntensity 0x70E360 while curtained or aimed at by an
    airstrike) and the slot loop 0x451F60 run natively over the building's
    slots (+0x55C): an anim whose type has ShouldUseCellDrawer (+0x35C), one
    whose type has not, and nineteen empty slots. The result: the building's
    +0x700 and each anim's drawer (+0xD4, the fixture Convert once written)
    and intensity (+0xFC, -12345 until written)."""
    emu = Emu()
    emu.write32(FRAME, frame)
    this = tint_building(emu, curtain=curtain, stage=stage, tint=tint, flash=flash,
                         airstrike=airstrike)
    emu.write32(CURTAIN_VT + 0x1BC, STUB_TINT_GET_CELL)
    emu.hook(STUB_TINT_GET_CELL, lambda _e: TINT_CELL, 0)
    emu.uc.mem_write(TINT_CELL + 0x10A, struct.pack('<h', intensity))
    for index in range(21):
        emu.write32(this + 0x55C + 4 * index, 0)
    write8(emu, this + 0x6ED, 0)
    anims = []
    for index, cell_drawer in enumerate((True, False)):
        anim = TINT_SLOT_ANIMS + 0x200 * index
        kind = TINT_ANIM_TYPES + 0x400 * index
        write8(emu, kind + 0x35C, int(cell_drawer))
        emu.write32(anim + 0xC8, kind)
        emu.write32(anim + 0xD4, 0)
        emu.write32(anim + 0xFC, -12345)
        emu.write32(this + 0x55C + 4 * index, anim)
        anims.append(anim)
    uc = emu.uc
    uc.reg_write(UC_X86_REG_ESP, STACK_BASE + STACK_SIZE - 0x1000)
    uc.reg_write(UC_X86_REG_ESI, this)
    uc.reg_write(UC_X86_REG_EAX, TINT_CONVERT)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    run_checked(uc, 0x450A47, 0x450A77, count=10_000)
    return dict(intensity=intensity, curtain=list(curtain), stage=stage, tint=list(tint),
                flash=flash, airstrike=airstrike, frame=frame,
                building_light=struct.unpack('<h', uc.mem_read(this + 0x700, 2))[0],
                drawer_set=[emu.read32(anim + 0xD4) == TINT_CONVERT for anim in anims],
                anim_light=[emu.read_i32(anim + 0xFC) for anim in anims])


def building_anim_light():
    """The curtain's edges, each stage's scale at three cell intensities, the
    flash count's bit 1, an airstrike aimed at the building or elsewhere off
    the curtain, and cell intensities at 0, the cap and below 0."""
    row = building_anim_light_row
    rows = [row(curtain=curtain) for curtain in CURTAIN_EDGES]
    for stage, tint in CURTAIN_STAGE_TINTS:
        rows += [row(stage=stage, tint=tint, intensity=intensity)
                 for intensity in (1000, 1500, 300)]
    rows += [row(flash=flash, intensity=intensity) for flash in (2, 3)
             for intensity in (1000, 1600)]
    rows += [row(curtain=(-1, 0), airstrike=airstrike) for airstrike in ('self', 'other')]
    rows += [row(curtain=(-1, 0), intensity=intensity) for intensity in (0, 2000, -5, -1000)]
    return rows


# DrawSHP's flags for a building's DrawBody pieces (0x6E00: Z read and write),
# its other callers (0x2E00), and AnimClass::DrawIt's and the voxel cache
# blit's (0x2800: Z read).
BLIT_FLAGS = (0x6E00, 0x2E00, 0x2800)
# The 16-bit ConvertClass constructor 0x48E740's blitter in each field the
# pickers answer for BLIT_FLAGS (RTTI class names), and where it stores them.
BLITTERS = ((0x98, 0x7E56F0),   # BlitTransXlatAlphaZRead<WORD>, 0x48F721
            (0xBC, 0x7E5630),   # BlitTransXlatAlphaZReadWrite<WORD>, 0x48F9AC
            (0x138, 0x7E5420),  # RLEBlitTransXlatAlphaZRead<WORD>, 0x490058
            (0x158, 0x7E53A0))  # RLEBlitTransXlatAlphaZReadWrite<WORD>, 0x4902A3
BLIT_WORDS = (0, 0x0018, 0xE000, 0xE798, 0xABCD0018)
# One line of four pixels: a hole, drawn, behind the Z line, drawn.
LINE_SOURCE = (0, 5, 200, 77)
LINE_RLE = (5, 0, 1, 200, 77)
LINE_Z = (0x1000, 0x1000, 0x0005, 0x1000)
LINE_A = (3, 3, 3, 9)
LINE_Z_VALUE = 0x100


def blit_pickers():
    """ConvertClass's blitter pickers for each of BLIT_FLAGS: 0x490B90 for an
    uncompressed frame and 0x490E50 for a compressed one (CC_Draw_Shape
    0x4AED70 picks by the frame's compression bit, 0x69E900), run natively on
    a fixture Convert whose fields hold their own offsets (+8 is not zero, so
    the lazy initializer 0x48EBF0 never runs). A row is [flags, the field
    0x490B90 answers, the field 0x490E50 answers]."""
    emu = Emu()
    for offset in range(0, 0x200, 4):
        emu.write32(TINT_CONVERT + offset, offset)
    return [[flags, emu.invoke(0x490B90, ecx=TINT_CONVERT, args=[flags]),
             emu.invoke(0x490E50, ecx=TINT_CONVERT, args=[flags])] for flags in BLIT_FLAGS]


def blitter_row(field, vtable, *, word, intensity=1000):
    """The blitter of Convert field `field` (vtable `vtable`) drawing the LINE_*
    line twice, each time on fresh lines: its plain copy (vt+4), then its
    tinted copy (vt+8) with the colour word `word`. A standard blitter takes
    (destination, source, count, Z value, Z line, A line, intensity, 0[,
    word]), an RLE one (destination, source, count, skip 0, Z value, Z line, A
    line, intensity, 0, Z-adjust line[, word]). Its conversion table (+4: 64K
    words) and row table (+8, the 0x420140 LUT: 255 rows of 256 row bases)
    hold arbitrary distinct words; the ZBuffer and ABuffer instances wrap far
    past these lines. The result: each copy's destination words (0xAAAA where
    it drew nothing) and Z line."""
    emu = Emu()
    emu.uc.mem_map(BLIT_TABLES, BLIT_TABLES_SIZE)
    conv, rows = BLIT_TABLES, BLIT_TABLES + 0x20000
    emu.uc.mem_write(conv, struct.pack('<65536H', *[(i * 0x9E37 + 0x1357) & 0xFFFF
                                                     for i in range(65536)]))
    emu.uc.mem_write(rows, struct.pack('<65280H', *[((q * 7 + a) & 0xFF) << 8
                                                     for q in range(255) for a in range(256)]))
    emu.write32(TINT_BLITTER, vtable)
    emu.write32(TINT_BLITTER + 4, conv)
    emu.write32(TINT_BLITTER + 8, rows)
    for instance, pointer in ((TINT_ZBUFFER, ZBUFFER_PTR), (TINT_ABUFFER, ABUFFER_PTR)):
        emu.write32(instance + 0x1C, 0x7FFFFFFF)
        emu.write32(instance + 0x20, 0x1000)
        emu.write32(pointer, instance)
    rle = field in (0x138, 0x158)
    dest, source, zline, aline, adjust = (TINT_LINES + 0x40 * k for k in range(5))
    out = {}
    for name, slot in (('plain', 4), ('tinted', 8)):
        emu.uc.mem_write(dest, struct.pack('<4H', *[0xAAAA] * 4))
        emu.uc.mem_write(source, bytes(LINE_RLE if rle else LINE_SOURCE))
        emu.uc.mem_write(zline, struct.pack('<4H', *LINE_Z))
        emu.uc.mem_write(aline, struct.pack('<4H', *LINE_A))
        emu.uc.mem_write(adjust, bytes(4))
        if rle:
            args = [dest, source, 4, 0, LINE_Z_VALUE, zline, aline, intensity, 0, adjust]
        else:
            args = [dest, source, 4, LINE_Z_VALUE, zline, aline, intensity, 0]
        emu.invoke(emu.read32(vtable + slot), ecx=TINT_BLITTER,
                   args=args + ([word] if slot == 8 else []))
        out[name] = list(struct.unpack('<4H', emu.uc.mem_read(dest, 8)))
        out[name + '_z'] = list(struct.unpack('<4H', emu.uc.mem_read(zline, 8)))
    return dict(field=field, vtable=vtable, word=word, intensity=intensity, **out)


def blitters():
    """Each of BLITTERS' plain and tinted copies for each of BLIT_WORDS."""
    return [blitter_row(field, vtable, word=word) for field, vtable in BLITTERS
            for word in BLIT_WORDS]



# ---------------------------------------------------------------- nuke impact

NUKE_FIXTURE = BASE + 0x3F8000
NUKE_BULLET = NUKE_FIXTURE
NUKE_BULLET_VT = NUKE_FIXTURE + 0x800
NUKE_WARHEAD = NUKE_FIXTURE + 0x1000
NUKE_ANIM_TYPE_ITEMS = NUKE_FIXTURE + 0x1800
NUKE_ANIM_TYPES = NUKE_FIXTURE + 0x2000
NUKE_HOLDER_ITEMS = NUKE_FIXTURE + 0x3000
NUKE_HOLDER_VT = NUKE_FIXTURE + 0x3800
NUKE_OTHER_HOLDER = NUKE_FIXTURE + 0x4000
NUKE_ANIM = NUKE_FIXTURE + 0x4800
STUB_NUKE_MARK = STUBS + 0x900
STUB_NUKE_UNINIT = STUBS + 0x910
STUB_NUKE_FIND = STUBS + 0x920
ANIM_TYPE_COUNT = 0x8B4160
# The anim-holder list (a DynamicVectorClass: vtable, items +4, capacity +8,
# IsAllocated +0xD, count +0x10, growth +0x14) PointerExpired walks for an
# expiring anim (0x725A2E).
ANIM_HOLDERS = 0xB0F5B8
# ObjectClass's bridge deck height, 416 as its initializer leaves it.
DECK_OFFSET = 0xAC13BC
DECK_LEPTONS = 416
# BulletClass: IsOnMap +0x74, OnBridge +0x8C, IsAlive +0x90, Location +0x9C,
# Warhead +0x128, the committed cell +0x14C, NextAnim +0x154, its wait +0x158.
B_ON_MAP, B_ON_BRIDGE, B_ALIVE, B_LOCATION = 0x74, 0x8C, 0x90, 0x9C
B_WARHEAD, B_CELL, B_NEXT_ANIM, B_WAITS = 0x128, 0x14C, 0x154, 0x158
NUKE_FLASH_START, NUKE_FLASH_DURATION = 0x827FC8, 0x827FCC


def nuke_bullet(emu, *, warhead='NUKE', location=(0, 0, 0), ground=0, on_bridge=False,
                on_map=True):
    """A fixture bullet whose vtable holds the native GetHeight 0x5F5F40,
    SetHeight 0x5F5FA0 and GetMapCoords 0x41BEA0, with Mark vt+0x124 and
    UnInit vt+0xF8 recorded stubs; the floor height 0x578080 answers
    `ground` and records the coordinate it was asked for."""
    emu.write32(NUKE_BULLET, NUKE_BULLET_VT)
    for slot, target in ((0x1C8, 0x5F5F40), (0x1CC, 0x5F5FA0), (0x1B8, 0x41BEA0),
                         (0x124, STUB_NUKE_MARK), (0xF8, STUB_NUKE_UNINIT)):
        emu.write32(NUKE_BULLET_VT + slot, target)
    emu.hook(STUB_NUKE_MARK, lambda e: e.events.append(['mark', e.arg(0)]), 4)
    emu.hook(STUB_NUKE_UNINIT, lambda e: e.events.append(['uninit']), 0)
    write_coord(emu, NUKE_BULLET + B_LOCATION, location)
    write8(emu, NUKE_BULLET + B_ON_BRIDGE, on_bridge)
    write8(emu, NUKE_BULLET + B_ON_MAP, on_map)
    write8(emu, NUKE_BULLET + B_ALIVE, 1)
    emu.write32(NUKE_BULLET + B_WARHEAD, NUKE_WARHEAD)
    emu.uc.mem_write(NUKE_WARHEAD + 0x24, warhead.encode().ljust(0x18, b'\0'))
    emu.write32(DECK_OFFSET, DECK_LEPTONS)

    def floor(e):
        e.events.append(['floor', read_coord(e, e.arg(0))])
        return ground

    emu.hook(0x578080, floor, 4)


NUKE_HOLDER_NAMES = {NUKE_BULLET: 'bullet', NUKE_OTHER_HOLDER: 'other'}


def nuke_holders(emu, holders):
    """The holder list with `holders` (names) and room for four; its Find
    vt+0x10 is a stub answering the item's index or -1."""
    addresses = {name: address for address, name in NUKE_HOLDER_NAMES.items()}
    emu.write32(ANIM_HOLDERS, NUKE_HOLDER_VT)
    emu.write32(NUKE_HOLDER_VT + 0x10, STUB_NUKE_FIND)
    emu.write32(ANIM_HOLDERS + 4, NUKE_HOLDER_ITEMS)
    emu.write32(ANIM_HOLDERS + 8, 4)
    write8(emu, ANIM_HOLDERS + 0xD, 1)
    emu.write32(ANIM_HOLDERS + 0x10, len(holders))
    emu.write32(ANIM_HOLDERS + 0x14, 10)
    for index, name in enumerate(holders):
        emu.write32(NUKE_HOLDER_ITEMS + 4 * index, addresses[name])

    def find(e):
        wanted = e.read32(e.arg(0))
        items = [e.read32(NUKE_HOLDER_ITEMS + 4 * index)
                 for index in range(e.read_i32(ANIM_HOLDERS + 0x10))]
        return items.index(wanted) if wanted in items else 0xFFFFFFFF

    emu.hook(STUB_NUKE_FIND, find, 4)


def nuke_holders_now(emu):
    return [NUKE_HOLDER_NAMES.get(emu.read32(NUKE_HOLDER_ITEMS + 4 * index), 'unknown')
            for index in range(emu.read_i32(ANIM_HOLDERS + 0x10))]


def nuke_impact_row(*, warhead='NUKE', height=-40, ground=416, on_bridge=False, on_map=True,
                    types=('NUKEANIM', 'NUKEBALL'), impact_flag=1, holders=(),
                    xy=(10 * 256 + 100, 12 * 256 + 50),
                    candidate=(11 * 256 + 3, 13 * 256 + 200, 900)):
    """BulletClass::AI's impact tail 0x467E53..0x467FEE as a slice: EBP the
    bullet (at `height` above the floor, and the deck for an OnBridge one),
    [ESP+0x18] the impact flag, [ESP+0x24] the candidate the flight step
    committed. The NUKE test 0x410A40, GetHeight, SetHeight, GetMapCoords,
    FindIndex 0x427CB0 (over fixture AnimTypes) and 0x48ACE0 run natively;
    ScreenNukeFlash 0x53AB70, CreateRadarEvent 0x65FA70, the anim
    constructor 0x421EA0 and the handoff 0x468D80 are recorded stubs."""
    emu = Emu()
    deck = DECK_LEPTONS if on_bridge else 0
    location = (xy[0], xy[1], ground + deck + height)
    nuke_bullet(emu, warhead=warhead, location=location, ground=ground,
                on_bridge=on_bridge, on_map=on_map)
    nuke_holders(emu, holders)
    names = {}
    for index, name in enumerate(types):
        kind = NUKE_ANIM_TYPES + 0x100 * index
        emu.uc.mem_write(kind + 0x24, name.encode().ljust(0x18, b'\0'))
        emu.write32(NUKE_ANIM_TYPE_ITEMS + 4 * index, kind)
        names[kind] = name
    emu.write32(ANIM_TYPES, NUKE_ANIM_TYPE_ITEMS)
    emu.write32(ANIM_TYPE_COUNT, len(types))
    emu.hook(0x53AB70, lambda e: e.events.append(['screen_nuke_flash']), 0)
    emu.hook(0x65FA70, lambda e: e.events.append(
        ['radar', i32(e.uc.reg_read(UC_X86_REG_ECX)),
         list(struct.unpack('<hh', struct.pack('<I', e.arg(0))))]), 4)

    def anim(e):
        e.events.append(['anim', names.get(e.arg(0), hex(e.arg(0))), read_coord(e, e.arg(1)),
                         i32(e.arg(2)), i32(e.arg(3)), e.arg(4), i32(e.arg(5)),
                         e.arg(6) & 0xFF])
        return e.uc.reg_read(UC_X86_REG_ECX)

    emu.hook(0x421EA0, anim, 0x1C)
    emu.hook(0x468D80, lambda e: e.events.append(['detonate', e.arg(0) & 0xFF]), 4)
    uc = emu.uc
    sp = STACK_BASE + STACK_SIZE - 0x1000
    uc.mem_write(sp + 0x18, u32(impact_flag))
    write_coord(emu, sp + 0x24, candidate)
    emu.write32(NUKE_BULLET + B_CELL, 0x7FFF7FFF)
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_EBP, NUKE_BULLET)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    run_checked(uc, 0x467E53, 0x467FEE, count=200_000)
    anim_held = emu.read32(NUKE_BULLET + B_NEXT_ANIM)
    return dict(warhead=warhead, height=height, ground=ground, on_bridge=on_bridge,
                on_map=on_map, types=list(types), impact_flag=impact_flag,
                holders=list(holders), location=list(location), candidate=list(candidate),
                events=emu.events, location_after=read_coord(emu, NUKE_BULLET + B_LOCATION),
                next_anim=int(anim_held != 0), waits=read8(emu, NUKE_BULLET + B_WAITS),
                holders_after=nuke_holders_now(emu), cell=read_cell(emu, NUKE_BULLET + B_CELL))


def nuke_impact():
    """The retail NUKE payload below the floor, on it and above it, other
    spellings and warheads, a bridge, a bullet off the map's marks, the
    anim types without NUKEBALL or with it first or lower-case, a list
    already holding an object, and a cell at negative coordinates."""
    row = nuke_impact_row
    return [row(), row(warhead='Nuke', height=0), row(warhead='nuke', height=25),
            row(height=-1), row(on_bridge=True, height=-10), row(on_map=False),
            row(types=('NUKEANIM',)), row(types=('NUKEANIM',), impact_flag=0),
            row(types=('NUKEBALL',)), row(types=('NUKEANIM', 'nukeball')),
            row(warhead='NUKE2'), row(warhead='NukeMaker', impact_flag=0), row(warhead='NUK'),
            row(holders=('other',)), row(xy=(-300, -1), candidate=(-3, -257, 0)),
            row(xy=(255, 256))]


def nuke_wait_row(*, alive=True, waits=True, anim=True, holders=('bullet',)):
    """BulletClass::AI's head from 0x4666F2 (EBP the bullet) as a slice:
    ObjectClass::AI 0x5F3E70 (a recorded stub), the IsAlive and wait tests,
    the holder list's removal (Find vt+0x10 a stub, the compaction native),
    the handoff 0x468D80 and UnInit (recorded stubs). It stops where the AI
    returns (0x467FEE, or 0x466781 after UnInit) or goes on to the flight
    (0x466789)."""
    emu = Emu()
    nuke_bullet(emu)
    write8(emu, NUKE_BULLET + B_ALIVE, alive)
    write8(emu, NUKE_BULLET + B_WAITS, waits)
    emu.write32(NUKE_BULLET + B_NEXT_ANIM, NUKE_ANIM if anim else 0)
    nuke_holders(emu, holders)
    emu.hook(0x5F3E70, lambda e: e.events.append(['object_ai']), 0)
    emu.hook(0x468D80, lambda e: e.events.append(['detonate', e.arg(0) & 0xFF]), 4)
    uc = emu.uc
    uc.reg_write(UC_X86_REG_ESP, STACK_BASE + STACK_SIZE - 0x1000)
    uc.reg_write(UC_X86_REG_EBP, NUKE_BULLET)
    uc.reg_write(UC_X86_REG_ECX, NUKE_BULLET)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    end = run_checked(uc, 0x4666F2, (0x466781, 0x466789, 0x467FEE), count=10_000)
    return dict(alive=alive, waits=waits, anim=anim, holders=list(holders),
                end={0x466781: 'detonated', 0x466789: 'flies', 0x467FEE: 'returns'}[end],
                events=emu.events, waits_after=read8(emu, NUKE_BULLET + B_WAITS),
                holders_after=nuke_holders_now(emu))


def nuke_wait():
    row = nuke_wait_row
    return [row(), row(anim=False), row(waits=False, holders=()), row(alive=False),
            row(alive=False, anim=False), row(anim=False, holders=('other', 'bullet')),
            row(anim=False, holders=('bullet', 'other')), row(anim=False, holders=()),
            row(waits=False, anim=False, holders=())]


def nuke_flash_emu():
    """Set_Defaults' lighting in the Scenario, the flash reset as
    SuperWeaponEffects::ResetAll 0x539760 leaves it, and RecalcLighting
    0x53AD00, UpdateLighting 0x53C280 and the redraw 0x4F42F0 recorded."""
    emu = Emu()
    scenario_lighting_defaults(emu)
    emu.write32(G_NUKE_FLASH, 0)
    emu.write32(NUKE_FLASH_START, 0xFFFFFFFF)
    emu.write32(NUKE_FLASH_DURATION, 0xFFFFFFFF)

    def recalc(e):
        e.events.append(['recalc', i32(e.uc.reg_read(UC_X86_REG_ECX)),
                         i32(e.uc.reg_read(UC_X86_REG_EDX)), i32(e.arg(0)), i32(e.arg(1))])

    emu.hook(0x53AD00, recalc, 8)
    emu.hook(0x53C280, lambda e: e.events.append(['update_lighting']), 0)
    emu.hook(0x4F42F0, lambda e: e.events.append(['redraw', e.arg(0)]), 4)
    return emu


def nuke_flash_state(emu):
    return [emu.read_i32(G_NUKE_FLASH), emu.read_i32(NUKE_FLASH_START),
            emu.read_i32(NUKE_FLASH_DURATION)]


def nuke_flash_process(emu, frame):
    """The head of LightningStorm::Process (0x53A6C0..0x53A742) at `frame`."""
    emu.write32(FRAME, frame & 0xFFFFFFFF)
    emu.events = []
    uc = emu.uc
    uc.reg_write(UC_X86_REG_ESP, STACK_BASE + STACK_SIZE - 0x1000)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    run_checked(uc, 0x53A6C0, 0x53A742, count=1_000)
    return list(emu.events)


def nuke_flash_row(*, frame=4000, frames=60, timer=(77, 55), target=100):
    """ScreenNukeFlash 0x53AB70 at `frame` over a Scenario whose Timer_1248
    and ambient target hold `timer` and `target`, then Process's head once a
    frame for `frames` frames: a step is a frame whose flash changed or that
    called out."""
    emu = nuke_flash_emu()
    emu.write32(SCENARIO + SCN_TIMER, timer[0])
    emu.write32(SCENARIO + SCN_TIMER + 8, timer[1])
    emu.write32(SCENARIO + SCN_TARGET, target)
    emu.write32(FRAME, frame & 0xFFFFFFFF)
    emu.events = []
    emu.invoke(0x53AB70)
    start = dict(flash=nuke_flash_state(emu),
                 timer=[emu.read_i32(SCENARIO + SCN_TIMER),
                        emu.read_i32(SCENARIO + SCN_TIMER + 8)],
                 target=emu.read_i32(SCENARIO + SCN_TARGET), events=list(emu.events))
    steps, last = [], nuke_flash_state(emu)
    for offset in range(1, frames + 1):
        events = nuke_flash_process(emu, frame + offset)
        now = nuke_flash_state(emu)
        if now != last or events:
            steps.append([offset, *now, events])
        last = now
    return dict(frame=frame, frames=frames, timer=list(timer), target_before=target,
                start=start, steps=steps)


def nuke_flash_step_row(*, status, start, duration, frame):
    """Process's head once at `frame` with the flash as given."""
    emu = nuke_flash_emu()
    emu.write32(G_NUKE_FLASH, status)
    emu.write32(NUKE_FLASH_START, start)
    emu.write32(NUKE_FLASH_DURATION, duration)
    events = nuke_flash_process(emu, frame)
    return dict(status=status, start=start, duration=duration, frame=frame,
                after=nuke_flash_state(emu), events=events)


def nuke_flash():
    """The retail flash from start to off (and one starting where the
    timer's sum wraps), and single steps at each test's edges."""
    step = nuke_flash_step_row
    steps = [step(status=1, start=100, duration=30, frame=130),
             step(status=1, start=100, duration=30, frame=131),
             step(status=1, start=100, duration=-1, frame=5000),
             step(status=2, start=100, duration=15, frame=115),
             step(status=2, start=100, duration=15, frame=116),
             step(status=2, start=100, duration=-1, frame=5000),
             step(status=0, start=100, duration=15, frame=5000),
             step(status=3, start=100, duration=15, frame=5000),
             step(status=1, start=0x7FFFFFF0, duration=30, frame=0x7FFFFFF1),
             step(status=1, start=-50, duration=30, frame=-19),
             step(status=1, start=-50, duration=30, frame=-20)]
    return dict(runs=[nuke_flash_row(), nuke_flash_row(frame=17, timer=(-1, 0), target=150)],
                steps=steps)


NUKE_CHANGE_RATE_TOKENS = ('0', '1', '1.5', '2.99', '3', '10', '-1', '-2.5', '.5', '1000000',
                           '3000000000')


def nuke_lighting_read():
    """The map's NukeAmbientChangeRate= (Read_INI_Basic 0x68AAD5..0x68AAFD):
    Set_Defaults' value (+0x3578), the default slice (FILD, FSTP double
    [ESP]) and the answer through Math::ftol 0x7C5F00 (0x68AAF8), a
    trampoline standing in for ReadDouble as in dominator_lighting_read."""
    emu = Emu()
    uc = emu.uc
    scenario_lighting_defaults(emu)
    stored = emu.read_i32(SCENARIO + 0x3578)
    uc.reg_write(UC_X86_REG_ESP, STACK_BASE + STACK_SIZE - 0x1000)
    uc.reg_write(UC_X86_REG_ESI, SCENARIO)
    uc.reg_write(UC_X86_REG_EDI, 0)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    run_checked(uc, 0x68AAD5, 0x68AAE9, count=50)
    default = struct.unpack('<d', uc.mem_read(uc.reg_read(UC_X86_REG_ESP), 8))[0]
    authored = []
    for token in NUKE_CHANGE_RATE_TOKENS:
        value = struct.unpack('<f', struct.pack('<f', float(token)))[0]
        authored.append([token, converted(emu, value, 0x68AAF8, 0x68AAFD)])
    return dict(stored_default=stored, default_double=default,
                default_units=converted(emu, default, 0x68AAF8, 0x68AAFD), authored=authored)


# ------------------------------------------------------- force_shield_launch

TYPE_FORCE_SHIELD = 10
FORCE_SHIELD = BASE + 0x3B0000
FS_CELLS = FORCE_SHIELD
FS_CELL_STRIDE = 0x200
FS_CELL_VT = FORCE_SHIELD + 0x1000
# Houses 0x10 apart: IsAlliedWith reads only +0x30 (ArrayIndex) and +0x5788
# (the ally bits), which stay disjoint at this stride.
FS_HOUSES = FORCE_SHIELD + 0x2000
FS_HOUSE_STRIDE = 0x10
FS_BUILDINGS = FORCE_SHIELD + 0x8000
FS_BUILDING_STRIDE = 0x400
FS_BUILDING_VT = FORCE_SHIELD + 0xE000
FS_ITEMS = FORCE_SHIELD + 0xF000
FS_ANIM_TYPE = FORCE_SHIELD + 0xF800
FS_CELL_ARG = FORCE_SHIELD + 0xFC00

BUILDING_ARRAY_ITEMS = 0xA8EB44
BUILDING_ARRAY_COUNT = 0xA8EB50
STUB_FS_CELL_COORDS = STUBS + 0xB00
STUB_FS_BUILDING_COORDS = STUBS + 0xB10
STUB_FS_CURTAIN = STUBS + 0xB20

FS_START_SOUND = 41
FS_SPECIAL_SOUND = 37
FS_TARGET = (40, 40)
FS_CENTRE = (40 * 256 + 128, 40 * 256 + 128)


def fs_house(index):
    return FS_HOUSES + FS_HOUSE_STRIDE * index


def fs_house_index(address):
    return (address - FS_HOUSES) // FS_HOUSE_STRIDE


def install_fs_houses(emu, allies):
    """House k: ArrayIndex k (+0x30) and the row's ally bits (+0x5788)."""
    for index, bits in enumerate(allies):
        emu.write32(fs_house(index) + 0x30, index)
        emu.write32(fs_house(index) + 0x5788, bits)


def install_fs_buildings(emu, buildings):
    """BuildingClass::Array (0xA8EB44 items, 0xA8EB50 count) holding the
    row's buildings, each an owner house (+0x21C) whose GetCoords (vt+0x48)
    answers its coordinate and whose IronCurtain (vt+0x154) is recorded."""
    emu.write32(FS_BUILDING_VT + 0x48, STUB_FS_BUILDING_COORDS)
    emu.write32(FS_BUILDING_VT + 0x154, STUB_FS_CURTAIN)
    for index, (owner, _coords) in enumerate(buildings):
        this = FS_BUILDINGS + FS_BUILDING_STRIDE * index
        emu.write32(this, FS_BUILDING_VT)
        emu.write32(this + 0x21C, fs_house(owner))
        emu.write32(FS_ITEMS + 4 * index, this)
    emu.write32(BUILDING_ARRAY_ITEMS, FS_ITEMS)
    emu.write32(BUILDING_ARRAY_COUNT, len(buildings))

    def building(e):
        return (e.uc.reg_read(UC_X86_REG_ECX) - FS_BUILDINGS) // FS_BUILDING_STRIDE

    def coords(e):
        out = e.arg(0)
        write_coord(e, out, buildings[building(e)][1])
        return out

    def curtain(e):
        e.events.append(['curtain', building(e), i32(e.arg(0)), fs_house_index(e.arg(1)),
                         e.arg(2)])

    emu.hook(STUB_FS_BUILDING_COORDS, coords, 4)
    emu.hook(STUB_FS_CURTAIN, curtain, 0xC)


def record_play_at(emu):
    """VocClass::PlayAt 0x7509E0 (ECX the index, EDX the coordinate, one
    stack argument)."""
    emu.hook(0x7509E0, lambda e: e.events.append(
        ['play_at', i32(e.uc.reg_read(UC_X86_REG_ECX)),
         read_coord(e, e.uc.reg_read(UC_X86_REG_EDX)), e.arg(0)]), 4)


def force_shield_launch_row(*, buildings, allies=(1,), cell=FS_TARGET, level=0,
                            bridge=False, coords=None, charged=True, start_sound=FS_START_SOUND,
                            radius=4, duration=500, blackout=1000, fade=75, player=False):
    """Launch 0x6CC390 from its entry for a type whose Type= (+0xB4) is 10:
    case 10 (0x6CD072..0x6CD2EB) with BuildingClass::Array holding
    `buildings` ((owner house, GetCoords) each, house 0 launching), the walk's
    IsAlliedWith 0x4F9A50, CoordStruct 0x41C230 and Distance3D 0x41C380 run
    natively. Every map lookup answers the row's cell, whose GetCoords is its
    centre raised 104 leptons per level (or `coords`) and whose +0x140
    carries the bridge bit 0x100 when `bridge`."""
    emu = Emu()
    for initializer in (0x6CADC0, 0x6CADE0):
        emu.invoke(initializer)
    emu.write32(0xB0C07C, BRIDGE_HEIGHT)
    emu.write32(SUPER + 0x28, SW_TYPE)
    emu.write32(SUPER + 0x2C, fs_house(0))
    emu.write32(SUPER + 0x50, -1)
    write8(emu, SUPER + 0x6F, charged)
    emu.write32(SW_TYPE + 0xB4, TYPE_FORCE_SHIELD)
    emu.write32(SW_TYPE + 0xC4, start_sound)
    emu.write32(RULES + 0x34C, FS_ANIM_TYPE)
    emu.write32(RULES + 0x17B8, radius)
    emu.write32(RULES + 0x17BC, duration)
    emu.write32(RULES + 0x17C0, blackout)
    emu.write32(RULES + 0x17C4, fade)
    emu.write32(SELECTED_SUPER, 9)
    emu.uc.mem_write(FS_CELL_ARG, struct.pack('<hh', *cell))
    install_fs_houses(emu, allies)
    install_fs_buildings(emu, buildings)
    answer = coords or [cell[0] * 256 + 128, cell[1] * 256 + 128, level * LEVEL_LEPTONS]
    lookups = []

    def lookup(e):
        this = FS_CELLS + FS_CELL_STRIDE * len(lookups)
        lookups.append(this)
        e.write32(this, FS_CELL_VT)
        e.write32(this + 0x140, 0x100 if bridge else 0)
        e.events.append(['cell', read_cell(e, e.arg(0))])
        return this

    def anim(e):
        e.events.append(['anim', e.arg(0) == FS_ANIM_TYPE, read_coord(e, e.arg(1)),
                         [i32(e.arg(n)) for n in range(2, 7)]])
        return e.uc.reg_read(UC_X86_REG_ECX)

    emu.write32(FS_CELL_VT + 0x48, STUB_FS_CELL_COORDS)
    emu.hook(0x5657A0, lookup, 4)
    emu.hook(STUB_FS_CELL_COORDS, coords_stub(answer), 4)
    emu.hook(0x421EA0, anim, 0x1C)
    record_play_at(emu)
    emu.hook(0x50BC90, lambda e: e.events.append(
        ['blackout', fs_house_index(e.uc.reg_read(UC_X86_REG_ECX)), i32(e.arg(0))]), 4)
    emu.hook(0x753250, lambda e: e.events.append(
        ['vox_find', read_name(e, e.uc.reg_read(UC_X86_REG_ECX))]) or 33, 0)
    emu.hook(0x752A40, lambda e: e.events.append(
        ['vox_remove', e.uc.reg_read(UC_X86_REG_ECX)]), 0)
    emu.invoke(0x6CC390, ecx=SUPER, args=[FS_CELL_ARG, int(player)])
    return dict(buildings=[[owner, list(at)] for owner, at in buildings], allies=list(allies),
                cell=list(cell), level=level, bridge=bridge, cell_coords=answer,
                charged=charged, start_sound=start_sound, radius=radius, duration=duration,
                blackout=blackout, fade=fade, player=player, events=emu.events,
                fade_countdown=emu.read_i32(SUPER + 0x50), fade_coords=read_coord(emu, SUPER + 0x54),
                selected_super=emu.read_i32(SELECTED_SUPER),
                sentinels=[read_coord(emu, 0xB0C020), read_coord(emu, 0xB0C070)])


def fs_near(dx=0, dy=0, dz=0, owner=0, level=0):
    """A building `dx, dy, dz` leptons from the target cell's centre."""
    return (owner, (FS_CENTRE[0] + dx, FS_CENTRE[1] + dy, level * LEVEL_LEPTONS + dz))


# The radius is 4 cells (1024 leptons): at, inside and outside it on each
# axis, across the diagonal truncation, in 3D, and far away.
FS_DISTANCES = (fs_near(), fs_near(dx=1023), fs_near(dx=1024), fs_near(dx=1025),
                fs_near(dy=-1023), fs_near(dy=-1024), fs_near(dx=724, dy=724),
                fs_near(dx=725, dy=725), fs_near(dx=768, dy=677), fs_near(dx=768, dy=678),
                fs_near(dx=1000, dz=300), fs_near(dx=900, dz=-416), fs_near(dx=-1000, dz=104),
                fs_near(dx=5120, dy=5120))
# House 0 launches. House 1 and it are mutual allies; house 2 lists house 0
# but not the reverse; house 0 lists house 3 but not the reverse; house 4 is
# no one's ally. Every building is within the radius.
FS_ALLIES = (0b01011, 0b00011, 0b00101, 0b01000, 0b10000)
FS_OWNERS = tuple(fs_near(dx=200 * owner, owner=owner) for owner in range(5))


def force_shield_launch():
    rows = [force_shield_launch_row(buildings=FS_DISTANCES),
            force_shield_launch_row(buildings=FS_OWNERS, allies=FS_ALLIES),
            force_shield_launch_row(buildings=FS_OWNERS, allies=FS_ALLIES, player=True),
            force_shield_launch_row(buildings=FS_OWNERS, allies=FS_ALLIES, charged=False,
                                    player=True),
            force_shield_launch_row(buildings=FS_OWNERS, allies=FS_ALLIES, start_sound=-1),
            force_shield_launch_row(buildings=(), player=True)]
    # The bridge raises the anim, the stored coordinate and the sound, not
    # the walk's centre.
    for bridge in (False, True):
        rows.append(force_shield_launch_row(
            buildings=(fs_near(dx=1000), fs_near(dx=1000, dz=416), fs_near(dz=416)),
            bridge=bridge))
    for level in (1, 2):
        rows.append(force_shield_launch_row(
            buildings=(fs_near(dx=1000), fs_near(dx=1000, level=level),
                       fs_near(dx=900, dy=400)), level=level))
    for radius in (0, 1, 2, 10):
        rows.append(force_shield_launch_row(buildings=FS_DISTANCES, radius=radius))
    for duration, fade in ((500, 0), (75, 75), (100, 600), (1, 2), (0, 0)):
        rows.append(force_shield_launch_row(buildings=(fs_near(),), duration=duration,
                                            fade=fade))
    # The walk's sentinels: the zero coordinate and cell (0, 0)'s centre at
    # level 0 (the static initializers 0x6CADC0 and 0x6CADE0).
    origin = (0, (128, 128, 0))
    rows.append(force_shield_launch_row(buildings=(origin,), cell=(0, 0)))
    rows.append(force_shield_launch_row(buildings=(origin,), cell=(0, 0), level=1))
    rows.append(force_shield_launch_row(buildings=((0, (0, 0, 0)),), coords=[0, 0, 0]))
    rows.append(force_shield_launch_row(buildings=((0, (128, 128, 0)),), cell=(0, 0),
                                        bridge=True))
    return rows


# ---------------------------------------------------------------- super_fade

FS_FADE_COORDS = (10368, 10368, 416)


def super_fade_row(*, start, calls):
    """SuperClass::AI 0x6CBCA0 called `calls` times on a Super whose +0x50
    starts at `start`: its head (0x6CBCA8..0x6CBCD4) and, ungranted
    (+0x6D clear), the early return 0x6CBE9D. The value after each call and
    every PlayAt."""
    emu = Emu()
    emu.write32(SUPER + 0x28, SW_TYPE)
    emu.write32(SW_TYPE + 0xC0, FS_SPECIAL_SOUND)
    emu.write32(SUPER + 0x50, start)
    write_coord(emu, SUPER + 0x54, FS_FADE_COORDS)
    write8(emu, SUPER + 0x6D, 0)
    emu.write32(SUPER + 0x68, 0)
    emu.write32(SELECTED_SUPER, -1)
    record_play_at(emu)
    values = []
    plays = []
    for call in range(1, calls + 1):
        before = len(emu.events)
        emu.invoke(0x6CBCA0, ecx=SUPER, args=[0])
        values.append(emu.read_i32(SUPER + 0x50))
        plays += [[call] + event[1:] for event in emu.events[before:]]
    return dict(start=start, calls=calls, values=values, plays=plays)


def super_fade():
    """Case 10 leaves 425 under retail rules (ForceShieldDuration=500 less
    ForceShieldPlayFadeSoundTime=75); the others are the edges."""
    return [super_fade_row(start=start, calls=calls)
            for start, calls in ((425, 428), (3, 6), (1, 3), (0, 2), (-1, 2), (-100, 2),
                                 (-2147483648, 2))]


# ---------------------------------------------------------------- lightning storm

STORM = BASE + 0x3A0000
STORM_CELLS = STORM
STORM_CELL_STRIDE = 0x200
STORM_ANIM_VT = STORM + 0x6000
STORM_TYPE_VT = STORM + 0x6400
STORM_OBJECT_VT = STORM + 0x6800
STORM_TYPES = STORM + 0x7000
STORM_TYPE_STRIDE = 0x80
STORM_IMAGES = STORM + 0x9000
STORM_LIST_ITEMS = STORM + 0xA000
STORM_RULE_ITEMS = STORM + 0xB000
STORM_OBJECTS = STORM + 0xC000
STORM_WARHEAD = STORM + 0xD000
STORM_TEXT = STORM + 0xE000
STORM_ARG = STORM + 0xF000

STUB_STORM_ANIM_TYPE = STUBS + 0xC00
STUB_STORM_ANIM_COORDS = STUBS + 0xC10
STUB_STORM_IMAGE = STUBS + 0xC20
STUB_STORM_WHAT = STUBS + 0xC30

G_STORM_TIME_TO_END = 0xA9FAD0
G_STORM_DEFERMENT = 0xA9FAB8
G_STORM_DURATION = 0x827FC4
G_STORM_START = 0x827FC0
G_STORM_COORDS = 0xA9F9CC
G_STORM_OWNER = 0xA9FACC
# DynamicVectorClass<AnimClass*> objects: items +4, capacity +8, count +0x10.
STORM_LISTS = (('present', 0xA9F9D0), ('manifesting', 0xA9FA60), ('bolts', 0xA9FA18))
PLAYER_PTR = 0xA83D4C
HOUSE_COUNT = 0xA80238
MUTE_LAUNCHES = 0xA8B538
# MapClass+0xF4/+0xF8, the map Size In_Bounds 0x568300 reads, and MapRect
# (+0x124..+0x130: left, top, width, height), which MapClass::Resize 0x565C10
# writes as (1, 1, W + H - 1, W + H - 1).
MAP_SIZE = MAP + 0xF4
MAP_RECT = MAP + 0x124
# The storm translation unit's CRT initializers (0x813560..0x8135A8, in table
# order): its copies of the level height (0xA9FA90) and bridge height
# (0xA9FA84), the empty cell (0xA9F9F8) and the zero coordinate (0xA9FA30).
STORM_INITIALIZERS = (0x539220, 0x539250, 0x539280, 0x5392B0, 0x5392C0, 0x5392E0, 0x539300,
                      0x539350, 0x539380, 0x5393A0, 0x5393C0, 0x5393E0, 0x539400, 0x539420,
                      0x539460, 0x539490, 0x5394C0, 0x5394F0, 0x539500)
# The pixel-to-lepton scale 0xB0CDD8 0x6D2120 reads (CRT entries 0x814E58,
# 0x814E68, 0x814EA4).
PIXEL_HEIGHT_INITIALIZERS = (0x6D1830, 0x6D18C0, 0x6D1BF0)

STORM_RULES = {'deferment': 0x1794, 'damage': 0x1798, 'duration': 0x179C, 'hit_delay': 0x17A0,
               'scatter_delay': 0x17A4, 'spread': 0x17A8, 'separation': 0x17AC}
# Rules vectors the storm reads: items, count.
STORM_RULE_VECTORS = {'clouds': (0x2C0, 0x2CC), 'bolts': (0x2DC, 0x2E8),
                      'sounds': (0x738, 0x744), 'debris': (0x140, 0x14C)}
STORM_SOUND = 0x730
STORM_PRINT_TEXT = 0x17B0
STORM_WARHEAD_PTR = 0x17B4
# Retail [General]: LightningDeferment=250, LightningDamage=250,
# LightningStormDuration=180, LightningHitDelay=10, LightningScatterDelay=5,
# LightningCellSpread=10, LightningSeparation=3.
STORM_RETAIL = dict(deferment=250, damage=250, duration=180, hit_delay=10, scatter_delay=5,
                    spread=10, separation=3)
# Clouds and bolts: (name, SHP height, SHP frames).
STORM_CLOUDS = (('WCCLOUD1', 80, 20), ('WCCLOUD2', 80, 21))
STORM_BOLTS = (('WCLBOLT1', 381, 10), ('WCLBOLT2', 381, 11), ('WCLBOLT3', 381, 12))
STORM_DEBRIS = ('DBRIS1SM', 'DBRIS2SM', 'DBRIS3SM')
STORM_SOUNDS = (17,)
STORM_STORM_SOUND = 23
STORM_COLOR = 5


def storm_alloc(emu, size):
    address = emu.heap
    emu.heap += (size + 15) & ~15
    return address


class StormFixture:
    """A fresh image after the storm unit's and the pixel scale's
    initializers, with Rules' storm keys, fixture anim types (their image an
    SHP header: +4 height, +6 frames), cells, anims and houses. Each anim
    is a fixture AnimClass: +0xAC its stage, vt+0x88 its type, vt+0x48 its
    coordinate."""

    def __init__(self, *, seed=7, frame=1000, rules=STORM_RETAIL, clouds=STORM_CLOUDS,
                 bolts=STORM_BOLTS, debris=STORM_DEBRIS, sounds=STORM_SOUNDS, print_text=True,
                 cells=None, size=(60, 60), houses=((1, False),), player=0,
                 explosion='WCBOLTX', mute=0):
        self.emu = emu = Emu()
        for initializer in STORM_INITIALIZERS + PIXEL_HEIGHT_INITIALIZERS:
            emu.invoke(initializer)
        emu.write32(FRAME, frame & 0xFFFFFFFF)
        seed_scenario_rng(emu, seed)
        for key, offset in STORM_RULES.items():
            emu.write32(RULES + offset, rules[key])
        write8(emu, RULES + STORM_PRINT_TEXT, print_text)
        emu.write32(RULES + STORM_WARHEAD_PTR, STORM_WARHEAD)
        emu.write32(RULES + STORM_SOUND, STORM_STORM_SOUND)
        self.types, self.type_names = {}, {}
        slot = 0
        for (name, height, frames) in (*clouds, *bolts, *((name, 0, 9) for name in debris),
                                       (explosion, 0, 9) if explosion else ('', 0, 0)):
            if not name:
                continue
            this = STORM_TYPES + STORM_TYPE_STRIDE * slot
            image = STORM_IMAGES + 0x10 * slot
            emu.write32(this, STORM_TYPE_VT)
            emu.write32(this + 0x40, image)
            emu.uc.mem_write(image, struct.pack('<hhhh', 0, 0, height, frames))
            self.types[name] = this
            self.type_names[this] = name
            slot += 1
        self.explosion = self.types.get(explosion, 0)
        for (key, names) in (('clouds', [c[0] for c in clouds]), ('bolts', [b[0] for b in bolts]),
                             ('debris', list(debris))):
            items, count = STORM_RULE_VECTORS[key]
            base = STORM_RULE_ITEMS + 0x100 * ('clouds', 'bolts', 'sounds', 'debris').index(key)
            for index, name in enumerate(names):
                emu.write32(base + 4 * index, self.types[name])
            emu.write32(RULES + items, base)
            emu.write32(RULES + count, len(names))
        items, count = STORM_RULE_VECTORS['sounds']
        base = STORM_RULE_ITEMS + 0x200
        for index, sound in enumerate(sounds):
            emu.write32(base + 4 * index, sound)
        emu.write32(RULES + items, base)
        emu.write32(RULES + count, len(sounds))
        emu.write32(STORM_TYPE_VT + 0x9C, STUB_STORM_IMAGE)
        emu.write32(STORM_ANIM_VT + 0x88, STUB_STORM_ANIM_TYPE)
        emu.write32(STORM_ANIM_VT + 0x48, STUB_STORM_ANIM_COORDS)
        emu.write32(STORM_OBJECT_VT + 0x2C, STUB_STORM_WHAT)
        for index, (_name, vector) in enumerate(STORM_LISTS):
            emu.write32(vector + 4, STORM_LIST_ITEMS + 0x400 * index)
            emu.write32(vector + 8, 0x100)
            emu.write32(vector + 0x10, 0)
        self.anims = {}
        self.cells = {}
        self.cell_specs = cells or {}
        self.houses = []
        for index, (bits, defeated) in enumerate(houses):
            this = storm_alloc(emu, 0x16100)
            emu.write32(this + 0x30, index)
            emu.write32(this + 0x5788, bits)
            write8(emu, this + 0x1F5, defeated)
            emu.write32(this + 0x16054, STORM_COLOR)
            self.houses.append(this)
        items = storm_alloc(emu, 4 * max(1, len(self.houses)))
        for index, this in enumerate(self.houses):
            emu.write32(items + 4 * index, this)
        emu.write32(HOUSE_ITEMS, items)
        emu.write32(HOUSE_COUNT, len(self.houses))
        emu.write32(PLAYER_PTR, 0 if player is None else self.houses[player])
        emu.write32(MUTE_LAUNCHES, mute)
        emu.write32(MAP_SIZE, size[0])
        emu.write32(MAP_SIZE + 4, size[1])
        extent = size[0] + size[1] - 1
        for index, value in enumerate((1, 1, extent, extent)):
            emu.write32(MAP_RECT + 4 * index, value)
        self.install_hooks()

    # -- fixture objects

    def house_index(self, address):
        return self.houses.index(address) if address in self.houses else None

    def anim(self, type_name, coords, stage):
        emu = self.emu
        this = storm_alloc(emu, 0x1C8)
        self.place_anim(this, self.types[type_name], coords, stage)
        return this

    def place_anim(self, this, type_address, coords, stage):
        emu = self.emu
        emu.write32(this, STORM_ANIM_VT)
        emu.write32(this + 0xAC, stage)
        emu.write32(this + 0x1C0, type_address)
        write_coord(emu, this + 0x1B0, coords)
        self.anims[this] = len(self.anims)

    def set_list(self, name, anims):
        vector = dict(STORM_LISTS)[name]
        items = self.emu.read32(vector + 4)
        for index, this in enumerate(anims):
            self.emu.write32(items + 4 * index, this)
        self.emu.write32(vector + 0x10, len(anims))

    def list_ids(self, name):
        vector = dict(STORM_LISTS)[name]
        items = self.emu.read32(vector + 4)
        return [self.anims[self.emu.read32(items + 4 * index)]
                for index in range(self.emu.read_i32(vector + 0x10))]

    def cell(self, x, y):
        key = (x, y)
        if key not in self.cells:
            spec = dict(level=0, bridge=False, land=0, floor=None, building=None, nearest=None)
            spec.update(self.cell_specs.get(key, {}))
            this = STORM_CELLS + STORM_CELL_STRIDE * len(self.cells)
            emu = self.emu
            emu.uc.mem_write(this + 0x24, struct.pack('<hh', x, y))
            emu.uc.mem_write(this + 0x11B, struct.pack('<b', spec['level']))
            emu.write32(this + 0x140, 0x100 if spec['bridge'] else 0)
            emu.write32(this + 0xEC, spec['land'])
            self.cells[key] = (this, spec)
            self.set_occupants(key, spec['building'], spec['nearest'])
        return self.cells[key][0]

    def set_occupants(self, key, building, nearest):
        """The cell's building (0x47C520) and nearest object (0x47C3D0), each
        None or a WhatAmI kind; a fixture object per kind and slot."""
        this, _spec = self.cells[key]
        for offset, kind, slot in ((0x1F0, building, 0), (0x1F4, nearest, 1)):
            if kind is None:
                self.emu.write32(this + offset, 0)
                continue
            index = list(self.cells).index(key)
            obj = STORM_OBJECTS + 0x40 * (4 * index + 2 * slot + (kind[1] if isinstance(kind, tuple) else 0))
            what = kind[0] if isinstance(kind, tuple) else kind
            self.emu.write32(obj, STORM_OBJECT_VT)
            self.emu.write32(obj + 0x10, WHAT[what])
            self.emu.write32(this + offset, obj)

    # -- stubs

    def install_hooks(self):
        emu = self.emu

        def cell_at(e):
            x, y = struct.unpack('<hh', bytes(e.uc.mem_read(e.arg(0), 4)))
            return self.cell(x, y)

        def cell_at_coord(e):
            x, y, _z = read_coord(e, e.arg(0))
            return self.cell(x >> 8, y >> 8)

        def floor(e):
            """0x47B3A0: the cell's (ECX) floor height at a point within it;
            Get_Center_Coords asks for (128, 128). The row's floor, else
            flat ground at its level."""
            this = e.uc.reg_read(UC_X86_REG_ECX)
            spec = next(spec for cell, spec in self.cells.values() if cell == this)
            e.events.append(['floor', list(struct.unpack('<ii', bytes(e.uc.mem_read(e.arg(0), 8))))])
            return spec['floor'] if spec['floor'] is not None else spec['level'] * LEVEL_LEPTONS

        def anim_ctor(e):
            this = e.uc.reg_read(UC_X86_REG_ECX)
            kind = e.arg(0)
            coords = read_coord(e, e.arg(1))
            self.place_anim(this, kind, coords, 0)
            e.events.append(['anim', self.anims[this], self.type_names.get(kind),
                             coords, [i32(e.arg(n)) for n in range(2, 7)]])
            return this

        def anim_coords(e):
            this = e.uc.reg_read(UC_X86_REG_ECX)
            out = e.arg(0)
            write_coord(e, out, read_coord(e, this + 0x1B0))
            return out

        def select_anim(e):
            e.events.append(['select_anim', i32(e.uc.reg_read(UC_X86_REG_ECX)),
                             e.uc.reg_read(UC_X86_REG_EDX) == STORM_WARHEAD, i32(e.arg(0)),
                             read_coord(e, e.arg(1))])
            return self.explosion

        def flash(e):
            e.events.append(['flash', i32(e.uc.reg_read(UC_X86_REG_ECX)),
                             e.uc.reg_read(UC_X86_REG_EDX) == STORM_WARHEAD,
                             [i32(e.arg(n)) for n in range(3)], e.arg(3) & 0xFF, e.arg(4)])

        def damage(e):
            coords = read_coord(e, e.uc.reg_read(UC_X86_REG_ECX))
            e.events.append(['damage', coords, i32(e.uc.reg_read(UC_X86_REG_EDX)), e.arg(0),
                             e.arg(1) == STORM_WARHEAD, e.arg(2), self.house_index(e.arg(3))])
            key = (coords[0] >> 8, coords[1] >> 8)
            spec = self.cells[key][1]
            if 'after' in spec:
                after = spec['after']
                self.set_occupants(key, after.get('building', spec['building']),
                                   after.get('nearest', spec['nearest']))
                if 'level' in after:
                    self.emu.uc.mem_write(self.cells[key][0] + 0x11B,
                                          struct.pack('<b', after['level']))

        def radar_event(e):
            x, y = struct.unpack('<hh', struct.pack('<I', e.arg(0)))
            e.events.append(['radar_event', e.uc.reg_read(UC_X86_REG_ECX), [x, y]])

        def text(e):
            e.events.append(['text', read_name(e, e.uc.reg_read(UC_X86_REG_ECX))])
            return STORM_TEXT

        emu.hook(0x5657A0, cell_at, 4)
        emu.hook(0x565730, cell_at_coord, 4)
        emu.hook(0x47B3A0, floor, 4)
        emu.hook(0x47C520, lambda e: e.read32(e.uc.reg_read(UC_X86_REG_ECX) + 0x1F0), 0)
        emu.hook(0x47C3D0, lambda e: e.read32(e.uc.reg_read(UC_X86_REG_ECX) + 0x1F4), 0xC)
        emu.hook(0x421EA0, anim_ctor, 0x1C)
        emu.hook(STUB_STORM_ANIM_TYPE, lambda e: e.read32(e.uc.reg_read(UC_X86_REG_ECX) + 0x1C0), 0)
        emu.hook(STUB_STORM_ANIM_COORDS, anim_coords, 4)
        emu.hook(STUB_STORM_IMAGE, lambda e: e.read32(e.uc.reg_read(UC_X86_REG_ECX) + 0x40), 0)
        emu.hook(STUB_STORM_WHAT, lambda e: e.read32(e.uc.reg_read(UC_X86_REG_ECX) + 0x10), 0)
        emu.hook(0x48A4F0, select_anim, 8)
        emu.hook(0x48A620, flash, 0x14)
        emu.hook(0x489280, damage, 0x10)
        record_play_at(emu)
        emu.hook(0x750920, lambda e: e.events.append(
            ['play_at_pos', i32(e.uc.reg_read(UC_X86_REG_ECX)), e.uc.reg_read(UC_X86_REG_EDX)]), 8)
        emu.hook(0x752700, lambda e: e.events.append(
            ['eva', read_name(e, e.uc.reg_read(UC_X86_REG_ECX))]), 4)
        emu.hook(0x734E60, text, 8)
        emu.hook(0x5D3BA0, lambda e: e.events.append(['message', i32(e.arg(3))]), 0x1C)
        emu.hook(0x65FA70, radar_event, 4)
        emu.hook(0x53C280, lambda e: e.events.append(['update_lighting']), 0)
        emu.hook(0x4F42F0, lambda e: None, 4)
        emu.hook(0x53AF40, lambda e: None, 0)
        emu.hook(0x53B560, lambda e: None, 0)

    # -- state

    def set_storm(self, *, active=False, time_to_end=False, deferment=0, duration=180, start=0,
                  coords=(40, 40), owner=0):
        emu = self.emu
        write8(emu, G_STORM_ACTIVE, active)
        write8(emu, G_STORM_TIME_TO_END, time_to_end)
        emu.write32(G_STORM_DEFERMENT, deferment)
        emu.write32(G_STORM_DURATION, duration)
        emu.write32(G_STORM_START, start)
        emu.uc.mem_write(G_STORM_COORDS, struct.pack('<hh', *coords))
        emu.write32(G_STORM_OWNER, 0 if owner is None else self.houses[owner])

    def storm(self):
        emu = self.emu
        return dict(active=bool(read8(emu, G_STORM_ACTIVE)),
                    time_to_end=bool(read8(emu, G_STORM_TIME_TO_END)),
                    deferment=emu.read_i32(G_STORM_DEFERMENT),
                    duration=emu.read_i32(G_STORM_DURATION), start=emu.read_i32(G_STORM_START),
                    coords=list(struct.unpack('<hh', bytes(emu.uc.mem_read(G_STORM_COORDS, 4)))),
                    owner=self.house_index(emu.read32(G_STORM_OWNER)),
                    lists={name: self.list_ids(name) for name, _vector in STORM_LISTS})

    def house_states(self):
        emu = self.emu
        return [[emu.read_i32(this + 0x2B0), emu.read_i32(this + 0x2B8), read8(emu, this + 0x5779)]
                for this in self.houses]

    def anim_table(self):
        return sorted([[index, self.type_names.get(self.emu.read32(this + 0x1C0)),
                        read_coord(self.emu, this + 0x1B0), self.emu.read_i32(this + 0xAC)]
                       for this, index in self.anims.items()])


def storm_start_row(*, duration=180, deferment=250, cell=(40, 40), owner=0, active=False,
                    current_deferment=0, current_duration=77, houses=((1, False),), player=0,
                    print_text=True, mute=0, size=(60, 60), seed=7, frame=1000):
    """LightningStorm::Start 0x539EB0 (ECX duration, EDX deferment, cell,
    owner) over the given storm, houses (ally bits, defeated) and map
    Size."""
    fx = StormFixture(seed=seed, frame=frame, houses=houses, player=player,
                      print_text=print_text, mute=mute, size=size)
    fx.set_storm(active=active, deferment=current_deferment, duration=current_duration,
                 start=frame - 500, coords=(7, 8), owner=None)
    record_draws(fx.emu)
    packed = struct.unpack('<I', struct.pack('<hh', *cell))[0]
    fx.emu.invoke(0x539EB0, ecx=duration, edx=deferment,
                  args=[packed, 0 if owner is None else fx.houses[owner]])
    return dict(duration=duration, deferment=deferment, cell=list(cell), owner=owner,
                active=active, current_deferment=current_deferment,
                current_duration=current_duration, houses=[list(h) for h in houses],
                player=player, print_text=print_text, mute=mute, size=list(size), seed=seed,
                frame=frame, events=fx.emu.events, storm=fx.storm(),
                house_states=fx.house_states(),
                rng_after=rng_state(fx.emu))


def storm_start():
    allied = ((0b00011, False), (0b00011, False), (0b00100, False), (0b01000, True),
              (0b10001, False))
    rows = [storm_start_row(),
            storm_start_row(current_deferment=100),
            storm_start_row(current_deferment=300),
            storm_start_row(current_deferment=250),
            storm_start_row(deferment=-5, current_deferment=10),
            storm_start_row(deferment=0, houses=allied),
            storm_start_row(deferment=0, houses=allied, owner=None),
            storm_start_row(deferment=0, houses=allied, owner=2),
            storm_start_row(deferment=0, houses=allied, mute=1),
            storm_start_row(deferment=0, houses=allied, print_text=False, player=None),
            storm_start_row(deferment=0, active=True, houses=allied),
            storm_start_row(deferment=250, active=True),
            storm_start_row(deferment=0, duration=-1, houses=allied),
            # The empty cell, never inside In_Bounds' diamond, is drawn again
            # over MapRect until a drawn cell lies in it; any other cell is
            # kept, in bounds or not.
            storm_start_row(cell=(0, 0)),
            storm_start_row(cell=(0, 0), size=(8, 8), seed=3),
            storm_start_row(deferment=0, cell=(0, 0), size=(4, 6), seed=11, houses=allied),
            storm_start_row(cell=(70, 70))]
    return rows


def storm_cloud_row(*, cell=(40, 40), level=0, bridge=False, bolt_height=381, seed=7,
                    present=0):
    """LightningStorm::CreateCloudBolt 0x53A140 for `cell`: the cloud's
    coordinate (its height from the first WeatherConBolts= type's image
    through 0x6D2120), the Scenario draw that picks the cloud, the anim and
    both lists."""
    bolts = ((STORM_BOLTS[0][0], bolt_height, STORM_BOLTS[0][2]), *STORM_BOLTS[1:])
    fx = StormFixture(seed=seed, bolts=bolts,
                      cells={cell: dict(level=level, bridge=bridge)})
    existing = [fx.anim('WCCLOUD1', (1000, 1000, 0), 3) for _ in range(present)]
    fx.set_list('present', existing)
    fx.set_list('manifesting', existing)
    packed = struct.unpack('<I', struct.pack('<hh', *cell))[0]
    fx.emu.invoke(0x53A140, args=[packed])
    return dict(cell=list(cell), level=level, bridge=bridge, bolt_height=bolt_height, seed=seed,
                present=present, events=fx.emu.events, storm=fx.storm(),
                rng_after=rng_state(fx.emu))


def storm_cloud():
    rows = [storm_cloud_row()]
    for level, bridge in ((1, False), (4, False), (0, True), (2, True), (-1, False)):
        rows.append(storm_cloud_row(level=level, bridge=bridge))
    for height in (0, 1, 2, 3, 120, 381, 382, 1000):
        rows.append(storm_cloud_row(bolt_height=height))
    rows += [storm_cloud_row(seed=seed) for seed in (1, 2, 3, 4, 5)]
    rows.append(storm_cloud_row(present=3, cell=(10, 60)))
    return rows


# Every half SHP height CreateCloudBolt can pass (a signed word halved).
PIXEL_HEIGHT_DOMAIN = (-16384, 16383)
# Control words the domain runs under: the oracle's 53-bit chop, the startup
# 53-bit nearest and 64-bit nearest.
PIXEL_HEIGHT_FPCWS = (NATIVE_FPCW, 0x027F, 0x037F)
# The scale's significand: [0xB0CDD8] = PIXEL_SCALE_SIGNIFICAND * 2**-50.
PIXEL_SCALE_SIGNIFICAND = 0x1BDFB59E463B4E


def pixel_height_under(emu, pixels, fpcw):
    """0x6D2120 (ECX the pixel count) under `fpcw`."""
    uc = emu.uc
    sp = STACK_BASE + STACK_SIZE - 0x1004
    uc.mem_write(sp, u32(RET_MAGIC))
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_FPCW, fpcw)
    uc.reg_write(UC_X86_REG_ECX, pixels & 0xFFFFFFFF)
    run_checked(uc, 0x6D2120, RET_MAGIC)
    return i32(uc.reg_read(UC_X86_REG_EAX))


def fnv1a64_i32(values):
    digest = 0xCBF29CE484222325
    for value in values:
        for byte in struct.pack('<i', value):
            digest = ((digest ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return digest


def storm_pixel_heights():
    """0x6D2120, ftol((pixels - 0.5) * [0xB0CDD8]) after its initializers,
    for sample pixel counts, and as an FNV-1a digest of the i32 results over
    the whole half-height domain. Under each control word every result
    equals the exact product (2 * pixels - 1) * significand * 2**-51
    truncated toward zero; the generator stops otherwise."""
    emu = Emu()
    for initializer in PIXEL_HEIGHT_INITIALIZERS:
        emu.invoke(initializer)
    scale = bytes(emu.uc.mem_read(0xB0CDD8, 8))
    bits = int.from_bytes(scale, 'little')
    if (bits & ((1 << 52) - 1)) | (1 << 52) != PIXEL_SCALE_SIGNIFICAND or bits >> 52 != 0x401:
        raise OracleError(f'pixel scale 0x{bits:016X} moved')
    pixels = list(range(0, 601)) + [1000, 2047, 4095, 16383, 32767, -1, -2, -100, -16384,
                                    1 << 28, 0x7FFFFFFF, -0x80000000]
    first, last = PIXEL_HEIGHT_DOMAIN
    domain = range(first, last + 1)
    digests = []
    for fpcw in PIXEL_HEIGHT_FPCWS:
        heights = [pixel_height_under(emu, px, fpcw) for px in domain]
        for px, height in zip(domain, heights):
            product = (2 * px - 1) * PIXEL_SCALE_SIGNIFICAND
            exact = abs(product) >> 51
            if height != (exact if product >= 0 else -exact):
                raise OracleError(f'0x6D2120({px}) under 0x{fpcw:04X} is {height}')
        digests.append(fnv1a64_i32(heights))
    if len(set(digests)) != 1:
        raise OracleError('0x6D2120 differs between control words')
    return dict(scale_bits=scale[::-1].hex(),
                heights=[[px, pixel_height_under(emu, px, NATIVE_FPCW)] for px in pixels],
                domain=list(PIXEL_HEIGHT_DOMAIN),
                fpcws=[f'0x{fpcw:04X}' for fpcw in PIXEL_HEIGHT_FPCWS],
                fnv1a64=f'0x{digests[0]:016x}')


def storm_strike_row(*, coords=(40 * 256 + 128, 40 * 256 + 128, 900), cell=None, sounds=STORM_SOUNDS,
                     debris=STORM_DEBRIS, explosion='WCBOLTX', seed=7, owner=0):
    """LightningStorm::GroundStrike 0x53A300 at `coords` over a fixture cell
    (level, bridge bit, land, building and nearest object before and after
    the damage): the bolt, the sound draw, the explosion's selection and z
    adjust (recorded), the flash and damage calls (recorded) and the debris."""
    key = (coords[0] >> 8, coords[1] >> 8)
    fx = StormFixture(seed=seed, sounds=sounds, debris=debris, explosion=explosion,
                      cells={key: cell or {}}, houses=((1, False), (2, False)))
    fx.set_storm(active=True, owner=owner)
    record_draws(fx.emu)
    fx.emu.invoke(0x53A300, args=list(coords))
    return dict(coords=list(coords), cell=storm_cell_json(cell or {}), sounds=list(sounds),
                debris=list(debris), explosion=explosion, seed=seed, owner=owner,
                events=fx.emu.events, bolts=fx.list_ids('bolts'),
                rng_after=rng_state(fx.emu))


def storm_cell_json(spec):
    out = {}
    for key, value in spec.items():
        if key == 'after':
            out[key] = storm_cell_json(value)
        elif isinstance(value, tuple):
            out[key] = list(value)
        else:
            out[key] = value
    return out


def storm_strike():
    rows = [storm_strike_row()]
    # The land types whose empty cell drops debris (1, 3, 4, 11) and the others.
    for land in range(0, 13):
        rows.append(storm_strike_row(cell=dict(land=land)))
    for level, bridge, floor in ((2, False, None), (0, True, None), (3, True, None),
                                 (1, False, 150)):
        rows.append(storm_strike_row(cell=dict(level=level, bridge=bridge, floor=floor)))
    # Occupants: a surviving building or unit drops none; one the damage
    # removes, or a changed level, drops debris; infantry nearest never.
    rows += [storm_strike_row(cell=dict(land=1, building='building', nearest='building')),
             storm_strike_row(cell=dict(land=0, building='building', nearest='building',
                                        after=dict(building=None, nearest=None))),
             storm_strike_row(cell=dict(land=1, nearest='unit')),
             storm_strike_row(cell=dict(land=0, nearest='unit', after=dict(nearest=None))),
             storm_strike_row(cell=dict(land=0, nearest='unit', after=dict(nearest=('unit', 1)))),
             storm_strike_row(cell=dict(land=1, nearest='infantry')),
             storm_strike_row(cell=dict(land=0, nearest='infantry', after=dict(nearest=None))),
             storm_strike_row(cell=dict(land=0, after=dict(level=1))),
             storm_strike_row(cell=dict(land=1, nearest='aircraft'))]
    rows += [storm_strike_row(sounds=()), storm_strike_row(sounds=(4, 5, 6)),
             storm_strike_row(cell=dict(land=1), debris=('DBRIS1SM',)),
             storm_strike_row(explosion=None), storm_strike_row(owner=None)]
    rows += [storm_strike_row(cell=dict(land=3), seed=seed) for seed in (1, 2, 3, 4, 5, 6)]
    return rows


def storm_process_row(*, frame, storm, anims=(), present=(), manifesting=(), bolts=(), seed=7,
                      rules=STORM_RETAIL, print_text=True, cells=None, size=(60, 60),
                      houses=((1, False), (2, False))):
    """LightningStorm::Process 0x53A6C0 once at `frame` (the nuke flash
    idle; PsychicDominator::Process 0x53AF40 and 0x53B560 stubbed) over the
    given storm and lists of fixture anims ((type, coordinate, stage) each),
    with CreateCloudBolt, GroundStrike and Start run natively."""
    fx = StormFixture(seed=seed, frame=frame, rules=rules, print_text=print_text, cells=cells,
                      size=size, houses=houses)
    fx.set_storm(**storm)
    made = [fx.anim(name, coords, stage) for name, coords, stage in anims]
    for name, indexes in (('present', present), ('manifesting', manifesting), ('bolts', bolts)):
        fx.set_list(name, [made[index] for index in indexes])
    record_draws(fx.emu)
    fx.emu.invoke(0x53A6C0)
    return dict(frame=frame, storm_before=storm_json(storm), anims_before=[
        [name, list(coords), stage] for name, coords, stage in anims],
        present=list(present), manifesting=list(manifesting), bolts=list(bolts), seed=seed,
        rules=dict(rules), print_text=print_text, cells=storm_cells_json(cells or {}),
        size=list(size), houses=[list(h) for h in houses], events=fx.emu.events,
        storm=fx.storm(), anims=fx.anim_table(), house_states=fx.house_states(),
        rng_after=rng_state(fx.emu))


def storm_json(storm):
    return {key: list(value) if isinstance(value, tuple) else value for key, value in storm.items()}


def storm_cells_json(cells):
    return [[list(key), storm_cell_json(spec)] for key, spec in cells.items()]


STORM_CENTRE = (40, 40)
CLOUD_Z = 1320  # the fixture bolt's 381-pixel image: 0x6D2120(190)


def cloud_at(x, y, stage=0, name='WCCLOUD1'):
    return (name, (x * 256 + 128, y * 256 + 128, CLOUD_Z), stage)


def storm_process():
    raging = dict(active=True, duration=180, start=900, coords=STORM_CENTRE, owner=0)
    row = storm_process_row
    rows = [
        # The countdown, its 225-frame line, and its end calling Start.
        row(frame=1001, storm=dict(deferment=3, duration=180, coords=STORM_CENTRE, owner=0)),
        row(frame=1001, storm=dict(deferment=226, duration=180, coords=STORM_CENTRE, owner=0)),
        row(frame=1001, storm=dict(deferment=451, duration=180, coords=STORM_CENTRE, owner=0)),
        row(frame=1001, storm=dict(deferment=226, duration=180, coords=STORM_CENTRE, owner=0),
            print_text=False),
        row(frame=1001, storm=dict(deferment=1, duration=180, coords=STORM_CENTRE, owner=0)),
        row(frame=1001, storm=dict(deferment=1, duration=-1, coords=STORM_CENTRE, owner=None)),
        row(frame=1001, storm=dict(deferment=0, duration=180, coords=STORM_CENTRE, owner=0)),
        row(frame=1001, storm=dict(deferment=-4, duration=180, coords=STORM_CENTRE, owner=0)),
        # Raging: both cadences, one, neither.
        row(frame=1000, storm=raging),
        row(frame=1005, storm=raging),
        row(frame=1003, storm=raging),
        row(frame=1010, storm=raging, seed=3),
        row(frame=1020, storm=raging, seed=4),
        # The duration: still raging at start + duration, ending one later;
        # -1 never ends.
        row(frame=1080, storm=dict(raging, start=900)),
        row(frame=1081, storm=dict(raging, start=900)),
        row(frame=100000, storm=dict(raging, duration=-1)),
        # Odd delays and spreads; a zero spread draws nothing.
        row(frame=1001, storm=raging, rules=dict(STORM_RETAIL, hit_delay=7, scatter_delay=3,
                                                 spread=7)),
        row(frame=1001, storm=raging, rules=dict(STORM_RETAIL, hit_delay=1, scatter_delay=1,
                                                 spread=0)),
        row(frame=1001, storm=raging, rules=dict(STORM_RETAIL, hit_delay=1, scatter_delay=1,
                                                 spread=1)),
        # The scatter's separation from each cloud present, its usable-area
        # test and its three tries.
        row(frame=1005, storm=raging, anims=(cloud_at(40, 40), cloud_at(42, 41), cloud_at(38, 39)),
            present=(0, 1, 2)),
        row(frame=1005, storm=raging, rules=dict(STORM_RETAIL, separation=30),
            anims=(cloud_at(40, 40),), present=(0,)),
        row(frame=1005, storm=raging, rules=dict(STORM_RETAIL, separation=0),
            anims=(cloud_at(40, 40),), present=(0,)),
        row(frame=1005, storm=dict(raging, coords=(2, 2))),
        row(frame=1005, storm=dict(raging, coords=(31, 31)), seed=9),
        row(frame=1005, storm=dict(raging, coords=(88, 88)), seed=5),
        # The lists: a manifesting cloud strikes once its stage passes half
        # its frames, last first; a present cloud leaves at its last frame; a
        # bolt leaves at half its frames.
        row(frame=1003, storm=raging,
            anims=(cloud_at(41, 40, 10), cloud_at(42, 40, 11), cloud_at(43, 40, 19),
                   cloud_at(44, 40, 18), cloud_at(45, 41, 12, 'WCCLOUD2'),
                   ('WCLBOLT1', (5000, 5000, 0), 4), ('WCLBOLT2', (5000, 5000, 0), 5)),
            manifesting=(0, 1, 4), present=(0, 1, 2, 3, 4), bolts=(5, 6),
            cells={(42, 40): dict(land=1), (45, 41): dict(land=3, level=2)}),
        # The end: clouds keep it raging; an empty list ends it the next
        # Process, whose countdown branch then runs.
        row(frame=1100, storm=dict(raging, time_to_end=True), anims=(cloud_at(41, 40, 3),),
            present=(0,), manifesting=(0,)),
        row(frame=1100, storm=dict(raging, time_to_end=True)),
        row(frame=1100, storm=dict(raging, time_to_end=True, deferment=5)),
        row(frame=1100, storm=dict(raging, time_to_end=True, deferment=1, coords=(20, 21))),
        row(frame=1100, storm=dict(time_to_end=True, deferment=1, coords=(20, 21), owner=0)),
        row(frame=1100, storm=dict(raging, time_to_end=True, deferment=1, coords=(20, 21)),
            anims=(cloud_at(41, 40, 3),), present=(0,)),
    ]
    return rows


def radar_outage_expiry_row(*, start, duration, frame):
    """HouseClass::Update's radar-outage block 0x4F8490..0x4F84D9 as a slice
    (ESI the house): the timer (+0x2B0 start, +0x2B8 duration) and the
    recheck byte +0x5779 after."""
    emu = Emu()
    house = storm_alloc(emu, 0x5800)
    emu.write32(house + 0x2B0, start)
    emu.write32(house + 0x2B8, duration)
    emu.write32(FRAME, frame & 0xFFFFFFFF)
    uc = emu.uc
    uc.reg_write(UC_X86_REG_ESI, house)
    uc.reg_write(UC_X86_REG_ESP, STACK_BASE + STACK_SIZE - 0x1000)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    run_checked(uc, 0x4F8490, 0x4F84D9, count=100)
    return [start, duration, frame, emu.read_i32(house + 0x2B0), emu.read_i32(house + 0x2B8),
            read8(emu, house + 0x5779)]


def radar_availability_row(*, start, duration, frame):
    """HouseClass 0x508DF0 for PlayerPtr with the Scenario's FreeRadar
    (+0x34A4) set, so the outage timer alone decides: the value it hands
    RadarClass 0x656DF0 (0x656DE0 answers neither)."""
    emu = Emu()
    house = storm_alloc(emu, 0x5800)
    emu.write32(PLAYER_PTR, house)
    emu.write32(house + 0x2B0, start)
    emu.write32(house + 0x2B8, duration)
    write8(emu, SCENARIO + 0x34A4, 1)
    emu.write32(FRAME, frame & 0xFFFFFFFF)
    emu.hook(0x656DE0, lambda e: 2, 0)
    emu.hook(0x656DF0, lambda e: e.events.append(e.arg(0) & 0xFF), 4)
    emu.invoke(0x508DF0, ecx=house)
    return [start, duration, frame, emu.events[0], read8(emu, house + 0x5779)]


def radar_outage():
    timers = ((-1, 0), (-1, 5), (1000, 180), (1000, 0), (1000, 1), (0x7FFFFFF0, 100))
    frames = (999, 1000, 1001, 1100, 1178, 1179, 1180, 1181, 5000, 0x7FFFFFF5, -5)
    return dict(expiry=[radar_outage_expiry_row(start=s, duration=d, frame=f)
                        for s, d in timers for f in frames],
                availability=[radar_availability_row(start=s, duration=d, frame=f)
                              for s, d in timers for f in frames])



# ------------------------------------------------------- iron_curtain_launch

TYPE_IRON_CURTAIN = 1
IRON_CURTAIN = BASE + 0x3D0000
IC_CELLS = IRON_CURTAIN
IC_CELL_STRIDE = 0x200
IC_CELL_VT = IRON_CURTAIN + 0x4000
IC_OBJECTS = IRON_CURTAIN + 0x5000
IC_OBJECT_STRIDE = 0x300
IC_OBJECT_VT = IRON_CURTAIN + 0xB000
IC_HOUSE = IRON_CURTAIN + 0xC000
IC_ANIM_TYPE = IRON_CURTAIN + 0xD000
IC_CELL_ARG = IRON_CURTAIN + 0xE000
STUB_IC_CELL_COORDS = STUBS + 0xD00
STUB_IC_CURTAIN = STUBS + 0xD10

IC_TARGET = (40, 40)
IC_DURATION = 750
ABSTRACT_FOOT = 0x4
IC_OFFSETS = ((0, 0), (1, 0), (1, -1), (0, -1), (-1, -1), (-1, 0), (-1, 1), (0, 1), (1, 1))


def ic_object(index):
    return IC_OBJECTS + IC_OBJECT_STRIDE * index


def ic_obj(offset=(0, 0), *, bridge=False, foot=True, latch=False, unlink=False):
    """An object in the target cell offset by `offset`, on its bridge (+0xE8)
    or ground (+0xE4) list; `foot` sets AbstractFlags 4 (+0x14), `latch` the
    warp latch (+0x27C), and `unlink` clears its +0x30 inside the call."""
    return dict(offset=list(offset), bridge=bridge, foot=foot, latch=latch, unlink=unlink)


def iron_curtain_launch_row(*, objects=(), target=IC_TARGET, bridges=(), levels=None,
                            charged=True, player=False, mute=False, duration=IC_DURATION):
    """Launch 0x6CC390 from its entry for a type whose Type= (+0xB4) is 1: case
    1 (0x6CCE64..0x6CD06F). MapClass::operator[] 0x5657A0 answers one fixture
    cell per coordinate: its GetCoords (vt+0x48) answers the centre raised 104
    leptons per level (`levels`, by offset), its +0x140 carries the bridge bit
    for the offsets in `bridges`, and its ground (+0xE4) and bridge (+0xE8)
    lists link the row's objects, in row order, through +0x30. Each object's
    IronCurtain (vt+0x154) is recorded."""
    levels = levels or {}
    emu = Emu()
    emu.invoke(0x6CAE00)
    emu.write32(0xB0C07C, BRIDGE_HEIGHT)
    emu.write32(SUPER + 0x28, SW_TYPE)
    emu.write32(SUPER + 0x2C, IC_HOUSE)
    write8(emu, SUPER + 0x6F, charged)
    emu.write32(SW_TYPE + 0xB4, TYPE_IRON_CURTAIN)
    emu.write32(RULES + 0x348, IC_ANIM_TYPE)
    emu.write32(RULES + 0xFE8, duration)
    emu.write32(SELECTED_SUPER, 9)
    emu.write32(MUTE_LAUNCHES, int(mute))
    emu.uc.mem_write(IC_CELL_ARG, struct.pack('<hh', *target))
    cells = {}

    def cell_at(looked):
        offset = (looked[0] - target[0], looked[1] - target[1])
        if offset not in cells:
            this = IC_CELLS + IC_CELL_STRIDE * len(cells)
            emu.write32(this, IC_CELL_VT)
            emu.write32(this + 0x140, 0x100 if offset in bridges else 0)
            cells[offset] = this
        return cells[offset]

    heads = {}
    for index, spec in enumerate(objects):
        this = ic_object(index)
        emu.write32(this, IC_OBJECT_VT)
        write8(emu, this + 0x14, ABSTRACT_FOOT if spec['foot'] else 0)
        write8(emu, this + 0x27C, spec['latch'])
        key = (tuple(spec['offset']), spec['bridge'])
        if key in heads:
            emu.write32(heads[key][-1] + 0x30, this)
            heads[key].append(this)
        else:
            heads[key] = [this]
    for (offset, bridge), listed in heads.items():
        cell = cell_at((target[0] + offset[0], target[1] + offset[1]))
        emu.write32(cell + (0xE8 if bridge else 0xE4), listed[0])

    def lookup(e):
        looked = read_cell(e, e.arg(0))
        e.events.append(['cell', looked])
        return cell_at(looked)

    def coords(e):
        out = e.arg(0)
        this = e.uc.reg_read(UC_X86_REG_ECX)
        offset = next(key for key, value in cells.items() if value == this)
        x, y = target[0] + offset[0], target[1] + offset[1]
        write_coord(e, out, [x * 256 + 128, y * 256 + 128,
                             levels.get(offset, 0) * LEVEL_LEPTONS])
        return out

    def curtain(e):
        this = e.uc.reg_read(UC_X86_REG_ECX)
        index = (this - IC_OBJECTS) // IC_OBJECT_STRIDE
        e.events.append(['curtain', index, i32(e.arg(0)), e.arg(1) == IC_HOUSE, i32(e.arg(2))])
        if objects[index]['unlink']:
            e.write32(this + 0x30, 0)

    def anim(e):
        e.events.append(['anim', e.arg(0) == IC_ANIM_TYPE, read_coord(e, e.arg(1)),
                         [i32(e.arg(n)) for n in range(2, 7)]])
        return e.uc.reg_read(UC_X86_REG_ECX)

    def vox_find(e):
        e.events.append(['vox_find', read_name(e, e.uc.reg_read(UC_X86_REG_ECX))])
        return 33

    emu.write32(IC_CELL_VT + 0x48, STUB_IC_CELL_COORDS)
    emu.write32(IC_OBJECT_VT + 0x154, STUB_IC_CURTAIN)
    emu.hook(0x5657A0, lookup, 4)
    emu.hook(STUB_IC_CELL_COORDS, coords, 4)
    emu.hook(STUB_IC_CURTAIN, curtain, 0xC)
    emu.hook(0x421EA0, anim, 0x1C)
    emu.hook(0x752700, lambda e: e.events.append(
        ['eva', read_name(e, e.uc.reg_read(UC_X86_REG_ECX)), i32(e.uc.reg_read(UC_X86_REG_EDX)),
         i32(e.arg(0))]), 4)
    emu.hook(0x65FA70, lambda e: e.events.append(
        ['radar_event', i32(e.uc.reg_read(UC_X86_REG_ECX)),
         list(struct.unpack('<hh', struct.pack('<I', e.arg(0))))]), 4)
    emu.hook(0x753250, vox_find, 0)
    emu.hook(0x752A40, lambda e: e.events.append(
        ['vox_remove', e.uc.reg_read(UC_X86_REG_ECX)]), 0)
    emu.invoke(0x6CC390, ecx=SUPER, args=[IC_CELL_ARG, int(player)])
    return dict(target=list(target), objects=list(objects),
                bridges=[list(offset) for offset in bridges],
                levels=[[list(offset), level] for offset, level in levels.items()],
                charged=charged, player=player, mute=mute, duration=duration,
                events=emu.events, selected_super=emu.read_i32(SELECTED_SUPER))


def iron_curtain_launch():
    # One object in every cell of the block, in table order, then a cell
    # holding two Foot and a building (AddContent appends a building).
    spread = tuple(ic_obj(offset) for offset in reversed(IC_OFFSETS))
    rows = [iron_curtain_launch_row(objects=spread),
            iron_curtain_launch_row(objects=spread, player=True),
            iron_curtain_launch_row(objects=spread, charged=False, player=True),
            iron_curtain_launch_row(objects=spread, mute=True),
            iron_curtain_launch_row(objects=(), player=True),
            iron_curtain_launch_row(objects=(ic_obj(), ic_obj(), ic_obj(foot=False)))]
    # The warp latch skips a Foot only; a building's byte there is not read.
    rows.append(iron_curtain_launch_row(objects=(
        ic_obj(latch=True), ic_obj(), ic_obj(foot=False, latch=True), ic_obj((1, 0), latch=True))))
    # A removal inside the call clears +0x30: the rest of that list is not
    # visited, the next cell's is.
    rows.append(iron_curtain_launch_row(objects=(
        ic_obj(), ic_obj(unlink=True), ic_obj(), ic_obj((1, 0)))))
    # A bridge cell walks its deck list (+0xE8) only; the target's bridge
    # raises the anim.
    on_both = (ic_obj(), ic_obj(bridge=True), ic_obj((1, 0)), ic_obj((1, 0), bridge=True),
               ic_obj((0, 1), bridge=True))
    rows.append(iron_curtain_launch_row(objects=on_both))
    rows.append(iron_curtain_launch_row(objects=on_both, bridges=((0, 0),)))
    rows.append(iron_curtain_launch_row(objects=on_both, bridges=((1, 0), (0, 1))))
    for level in (1, 4):
        rows.append(iron_curtain_launch_row(objects=(ic_obj(),), levels={(0, 0): level},
                                            bridges=((0, 0),) if level == 4 else ()))
    # The block wraps each word: a target at the map's edge.
    rows.append(iron_curtain_launch_row(objects=(ic_obj((-1, -1)), ic_obj((1, 1))),
                                        target=(0, 0)))
    for duration in (0, 1, -1, 0x7FFFFFFF):
        rows.append(iron_curtain_launch_row(objects=(ic_obj(),), duration=duration))
    return rows


# --------------------------------------------------------- curtain_overrides

CURTAIN_OVERRIDES = BASE + 0x3E0000
CO_TECHNO = CURTAIN_OVERRIDES
CO_VT = CURTAIN_OVERRIDES + 0x1000
CO_TYPE = CURTAIN_OVERRIDES + 0x2000
CO_EATER = CURTAIN_OVERRIDES + 0x3000
CO_PARASITE = CURTAIN_OVERRIDES + 0x4000
CO_HOUSE = CURTAIN_OVERRIDES + 0x5000
CO_C4 = CURTAIN_OVERRIDES + 0x6000
STUB_CO_TYPE = STUBS + 0xD20
STUB_CO_DAMAGE = STUBS + 0xD30

CO_STRENGTH = 125


def curtain_override_row(*, kind, organic=False, eaten=False, frame=1000, duration=IC_DURATION,
                         strength=CO_STRENGTH, damage_result=2):
    """InfantryClass::IronCurtain 0x522600 (`kind` 'infantry') or
    FootClass::IronCurtain 0x4DEAE0 ('foot') on a fixture Techno: its type
    (+0x6C0, and GetTechnoType vt+0x84) holds Strength (+0xA0) and Organic
    (+0xD97); ReceiveDamage vt+0x16C, the parasite's ExitUnit 0x62A4A0 and
    TechnoClass::IronCurtain 0x70E2B0 are recorded stubs. With `eaten`, the
    Foot's eater (+0x694) holds a ParasiteClass (+0x69C). The timers each
    writes ([start, left]; the middle word is a stale local)."""
    emu = Emu()
    emu.write32(FRAME, frame)
    emu.write32(RULES + 0xFA8, CO_C4)
    emu.write32(CO_TECHNO, CO_VT)
    emu.write32(CO_TECHNO + 0x6C0, CO_TYPE)
    emu.write32(CO_TYPE + 0xA0, strength)
    write8(emu, CO_TYPE + 0xD97, organic)
    emu.write32(CO_TECHNO + 0x694, CO_EATER if eaten else 0)
    emu.write32(CO_EATER + 0x69C, CO_PARASITE)
    for timer in (CO_PARASITE + 0x2C, CO_TECHNO + 0x6A0):
        write_coord(emu, timer, [-7, -7, -7])
    emu.write32(CO_VT + 0x84, STUB_CO_TYPE)
    emu.write32(CO_VT + 0x16C, STUB_CO_DAMAGE)

    def damage(e):
        e.events.append(['receive_damage', i32(e.read32(e.arg(0))), i32(e.arg(1)),
                         e.arg(2) == CO_C4, e.arg(3), e.arg(4) & 0xFF, e.arg(5) & 0xFF,
                         e.arg(6) == CO_HOUSE])
        return damage_result

    emu.hook(STUB_CO_TYPE, lambda e: CO_TYPE, 0)
    emu.hook(STUB_CO_DAMAGE, damage, 0x1C)
    emu.hook(0x62A4A0, lambda e: e.events.append(
        ['exit_unit', e.uc.reg_read(UC_X86_REG_ECX) == CO_PARASITE]), 0)
    emu.hook(0x70E2B0, lambda e: e.events.append(
        ['techno_curtain', e.uc.reg_read(UC_X86_REG_ECX) == CO_TECHNO, i32(e.arg(0)),
         e.arg(1) == CO_HOUSE, i32(e.arg(2))]) or 7, 0xC)
    entry = 0x522600 if kind == 'infantry' else 0x4DEAE0
    result = emu.invoke(entry, ecx=CO_TECHNO, args=[duration, CO_HOUSE, 0])
    timer = lambda address: [emu.read_i32(address), emu.read_i32(address + 8)]
    return dict(kind=kind, organic=organic, eaten=eaten, frame=frame, duration=duration,
                strength=strength, damage_result=damage_result, events=emu.events,
                result=i32(result), parasite_timer=timer(CO_PARASITE + 0x2C),
                foot_timer=timer(CO_TECHNO + 0x6A0))


def curtain_overrides():
    rows = []
    for strength in (125, 1, 0, 600):
        rows.append(curtain_override_row(kind='infantry', strength=strength))
    rows.append(curtain_override_row(kind='infantry', organic=True, eaten=True))
    for organic in (False, True):
        for eaten in (False, True):
            rows.append(curtain_override_row(kind='foot', organic=organic, eaten=eaten))
    rows.append(curtain_override_row(kind='foot', eaten=True, frame=0, duration=1))
    rows.append(curtain_override_row(kind='foot', organic=True, strength=300))
    return rows



# ------------------------------------------------------------ paradrop_launch

TYPE_PARADROP = 5
TYPE_AMER_PARADROP = 6
# Each row runs in its own emulator: the Chronosphere rows' region is free.
PARADROP = BASE + 0x300000
PD_CELL_ARG = PARADROP
PD_COORDS = PARADROP + 0x100
PD_CELLS = PARADROP + 0x1000
PD_CELL_STRIDE = 0x200
PD_LISTS = PARADROP + 0x4000
PD_INF_TYPES = PARADROP + 0x10000
PD_INF_TYPE_STRIDE = 0x1000
PD_PLANE_TYPE = PARADROP + 0x20000
PD_PLANE_TYPE_VT = PARADROP + 0x21000
PD_PLANE_TYPE_ITEMS = PARADROP + 0x22000
PD_INF_TYPE_ITEMS = PARADROP + 0x23000
PD_INF_TYPE_VT = PARADROP + 0x24000
PD_PLANE_VT = PARADROP + 0x25000
PD_INFANTRY_VT = PARADROP + 0x26000
PD_TARGET = PARADROP + 0x27000
PD_NAV_COM = PARADROP + 0x27100
PD_TEAM = PARADROP + 0x27200
PD_DROP_CELL = PARADROP + 0x27300
PD_DROP_CELL_VT = PARADROP + 0x27400
PD_PLANES = PARADROP + 0x30000
PD_PLANE_STRIDE = 0x1000
PD_INFANTRY = PARADROP + 0x40000
PD_INFANTRY_STRIDE = 0x800

STUB_PD_PLANE_CREATE = STUBS + 0xD40
STUB_PD_QUEUE = STUBS + 0xD50
STUB_PD_DESTINATION = STUBS + 0xD60
STUB_PD_TARGET = STUBS + 0xD70
STUB_PD_UNLIMBO = STUBS + 0xD80
STUB_PD_WHAT_AM_I = STUBS + 0xD90
STUB_PD_COMMENCE = STUBS + 0xDA0
STUB_PD_DELETE = STUBS + 0xDB0
STUB_PD_INF_CREATE = STUBS + 0xDC0
STUB_PD_LIMBO = STUBS + 0xDD0
STUB_PD_PLANE_COORDS = STUBS + 0xDE0
STUB_PD_CAN_ENTER = STUBS + 0xDF0
STUB_PD_SPAWN_PARACHUTED = STUBS + 0xE00
STUB_PD_CONCEAL = STUBS + 0xE10
STUB_PD_CELL_COORDS = STUBS + 0xE20
STUB_PD_DO_ACTION = STUBS + 0xE30

INFANTRY_TYPE_ITEMS = 0xA8E34C
WATER_SET = 0xAA0738
GAME_MODE = 0xA8B238
PDPLANE_INDEX = 3
PD_INF_INDEX = 4
PD_WATER_SET = 50
PD_TARGET_CELL = (40, 40)
PD_RADIUS = 1024
# The plane's Location for the missions and the drop: cell (40, 40), FlightLevel 1500.
PD_LOCATION = (40 * 256 + 128, 40 * 256 + 128, 1500)
# Each side's [General] lists: the InfantryType TypeList's and the IntVector's
# (items, count) fields.
PD_LIST_FIELDS = {'amer': ((0xC08, 0xC14), (0xC24, 0xC30)),
                  'ally': ((0xC40, 0xC4C), (0xC5C, 0xC68)),
                  'sov': ((0xC78, 0xC84), (0xC94, 0xCA0)),
                  'yuri': ((0xCB0, 0xCBC), (0xCCC, 0xCD8))}
# Retail's single entries; each type is the ArrayIndex (+0xDF8) of its own
# fixture InfantryTypeClass.
PD_RETAIL_LISTS = {'amer': ([0], [8]), 'ally': ([0], [6]), 'sov': ([1], [9]), 'yuri': ([2], [6])}
# The int after each list's last num: what reading past the vector finds.
PD_PAST_THE_VECTOR = 0x5EED


def install_paradrop_lists(emu, lists):
    types_at = PD_INF_TYPES
    items = PD_LISTS
    for side, (types, nums) in lists.items():
        (type_items, type_count), (num_items, num_count) = PD_LIST_FIELDS[side]
        emu.write32(RULES + type_items, items)
        emu.write32(RULES + type_count, len(types))
        for n, array_index in enumerate(types):
            emu.write32(items + 4 * n, types_at)
            emu.write32(types_at + 0xDF8, array_index)
            types_at += PD_INF_TYPE_STRIDE
        items += 0x80
        emu.write32(RULES + num_items, items)
        emu.write32(RULES + num_count, len(nums))
        for n, num in enumerate(list(nums) + [PD_PAST_THE_VECTOR]):
            emu.write32(items + 4 * n, num)
        items += 0x80


def paradrop_launch_row(*, kind=TYPE_PARADROP, charged=True, player=False, side=0,
                        plane_index=PDPLANE_INDEX, cells=None, nearby=(0, 0), lists=None):
    """Launch 0x6CC390 from its entry for a type whose Type= (+0xB4) is 5 or 6:
    case 5 (0x6CD2EE..0x6CD534) or 6 (0x6CD537..0x6CD66A) and the shared tail
    0x6CD500. MapClass::operator[] 0x5657A0 answers by `cells`: a cell named
    there is 'null', 'dummy' (0xABDC50) or a fixture CellClass whose tile
    (+0x38) is WaterSet (0xAA0738 = 50) plus the given offset, read by
    0x485060, which runs natively; an unnamed cell answers null.
    Find_Nearby_Passable_Cell 0x56DC20 answers `nearby`; it, the PDPLANE
    lookup 0x41CAA0, SendParadropPlanes 0x65E660 and the EVA calls are
    recorded stubs. `lists` gives each side's (types, nums)."""
    cells = dict(cells if cells is not None else {PD_TARGET_CELL: 14})
    lists = lists if lists is not None else PD_RETAIL_LISTS
    emu = Emu()
    emu.invoke(0x6CADB0)
    emu.write32(WATER_SET, PD_WATER_SET)
    emu.write32(SUPER + 0x28, SW_TYPE)
    emu.write32(SUPER + 0x2C, HOUSE)
    write8(emu, SUPER + 0x6F, charged)
    emu.write32(SW_TYPE + 0xB4, kind)
    emu.write32(HOUSE + 0x1E8, side)
    emu.write32(SELECTED_SUPER, 9)
    emu.uc.mem_write(PD_CELL_ARG, struct.pack('<hh', *PD_TARGET_CELL))
    install_paradrop_lists(emu, lists)
    fixture = {}

    def cell_for(coord):
        answer = cells.get(coord, 'null')
        if answer == 'null':
            return 0
        if answer == 'dummy':
            return DUMMY_CELL
        if coord not in fixture:
            this = PD_CELLS + PD_CELL_STRIDE * len(fixture)
            emu.write32(this + 0x38, PD_WATER_SET + answer)
            fixture[coord] = this
        return fixture[coord]

    def name(address):
        for coord, this in fixture.items():
            if this == address:
                return list(coord)
        return {0: 'null', DUMMY_CELL: 'dummy'}.get(address, hex(address))

    def find_type(e):
        e.events.append(['find_aircraft_type', read_name(e, e.uc.reg_read(UC_X86_REG_ECX))])
        return plane_index

    def lookup(e):
        coord = read_cell(e, e.arg(0))
        e.events.append(['map_cell', coord])
        return cell_for(tuple(coord))

    def nearby_stub(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != MAP:
            raise OracleError('Find_Nearby_Passable_Cell on another map')
        e.events.append(['nearby', read_cell(e, e.arg(1)), [i32(e.arg(n)) for n in range(2, 12)],
                         read_cell(e, e.arg(12)), e.arg(13) & 0xFF, e.arg(14) & 0xFF])
        e.uc.mem_write(e.arg(0), struct.pack('<hh', *nearby))
        return e.arg(0)

    def send(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != HOUSE:
            raise OracleError('SendParadropPlanes for another house')
        e.events.append(['send', i32(e.uc.reg_read(UC_X86_REG_EDX)), i32(e.arg(0)),
                         i32(e.arg(1)), name(e.arg(2)), i32(e.arg(3)), i32(e.arg(4)),
                         i32(e.arg(5))])
        return 1

    def vox_find(e):
        e.events.append(['vox_find', read_name(e, e.uc.reg_read(UC_X86_REG_ECX))])
        return 33

    emu.hook(0x41CAA0, find_type, 0)
    emu.hook(0x5657A0, lookup, 4)
    emu.hook(0x56DC20, nearby_stub, 0x3C)
    emu.hook(0x65E660, send, 0x18)
    emu.hook(0x753250, vox_find, 0)
    emu.hook(0x752A40, lambda e: e.events.append(
        ['vox_remove', e.uc.reg_read(UC_X86_REG_ECX)]), 0)
    emu.invoke(0x6CC390, ecx=SUPER, args=[PD_CELL_ARG, int(player)])
    return dict(kind=kind, charged=charged, player=player, side=side, plane_index=plane_index,
                cells=[[list(coord), answer] for coord, answer in cells.items()],
                nearby=list(nearby),
                lists={side: [list(types), list(nums)] for side, (types, nums) in lists.items()},
                events=emu.events, selected_super=emu.read_i32(SELECTED_SUPER))


def paradrop_launch():
    rows = [paradrop_launch_row(charged=False, player=True),
            paradrop_launch_row(kind=TYPE_AMER_PARADROP, charged=False, player=True)]
    for kind in (TYPE_PARADROP, TYPE_AMER_PARADROP):
        for player in (False, True):
            rows.append(paradrop_launch_row(kind=kind, player=player))
        rows.append(paradrop_launch_row(kind=kind, plane_index=-1, player=True))
        for answer in ('null', 'dummy'):
            rows.append(paradrop_launch_row(kind=kind, cells={PD_TARGET_CELL: answer},
                                            player=True))
        # A water target: the nearby cell replaces it only when it is real
        # land; (0, 0), the empty cell, is never looked up.
        for nearby, answer in (((45, 41), 14), ((45, 41), 0), ((45, 41), 'null'),
                               ((45, 41), 'dummy'), ((0, 0), 14), ((0, 7), 14), ((7, 0), 14)):
            rows.append(paradrop_launch_row(kind=kind, cells={PD_TARGET_CELL: 0, nearby: answer},
                                            nearby=nearby))
    # WaterSet's 14 tiles, and the tiles either side of them.
    for tile in (-1, 13, 14):
        rows.append(paradrop_launch_row(cells={PD_TARGET_CELL: tile, (45, 41): 14},
                                        nearby=(45, 41)))
    # Case 5's lists by the house's side (+0x1E8): 0 Ally, 2 Yuri, any other Soviet.
    for side in (1, 2, 3, -1, 4):
        rows.append(paradrop_launch_row(side=side))
    # The lists: Ally, Yuri and Amer send nothing unless their lengths match;
    # Soviet loops over its types and reads past its nums. A type whose
    # ArrayIndex is -1 sends nothing.
    def lists(**sides):
        merged = dict(PD_RETAIL_LISTS)
        merged.update(sides)
        return merged
    for ally in (([], []), ([0, 1], [6]), ([0], [6, 2]), ([0, 1], [6, 3]), ([-1, 0], [6, 2]),
                 ([0, 1, 2], [0, 1, -1])):
        rows.append(paradrop_launch_row(lists=lists(ally=ally)))
    for yuri in (([2, 0], [6]), ([2, 0], [6, 4])):
        rows.append(paradrop_launch_row(side=2, lists=lists(yuri=yuri)))
    for sov in (([], []), ([1, 0], [9]), ([1], [9, 4]), ([1, 0], [9, 4]), ([-1], [9])):
        rows.append(paradrop_launch_row(side=1, lists=lists(sov=sov)))
    for amer in (([0, 1], [8]), ([0, 1], [8, 2]), ([], [])):
        rows.append(paradrop_launch_row(kind=TYPE_AMER_PARADROP, lists=lists(amer=amer)))
    return rows


# ------------------------------------------------------- send_paradrop_planes

def pd_plane(index):
    return PD_PLANES + PD_PLANE_STRIDE * index


def pd_infantry(index):
    return PD_INFANTRY + PD_INFANTRY_STRIDE * index


def cargo(emu, plane):
    """A plane's PassengerList (+0x114): its count and its chain from +0x118
    through +0x30, as fixture infantry indices."""
    chain = []
    at = emu.read32(plane + 0x118)
    while at and len(chain) < 64:
        chain.append((at - PD_INFANTRY) // PD_INFANTRY_STRIDE)
        at = emu.read32(at + 0x30)
    return [emu.read_i32(plane + 0x114), chain]


def send_paradrop_planes_row(*, inf_index=PD_INF_INDEX, num=3, edge=-1, waypoint_edge=0,
                             created=True, unlimbo=True, what_am_i=2, infantry=None,
                             inf_type=True, pick=(30, 1)):
    """HouseClass::SendParadropPlanes 0x65E660 with case 5's arguments (one
    plane, mission 0x1A, the target cell, no destination, the list entry's
    InfantryType index and num). The plane's and the infantry's constructors
    (vt+0x8C), PickCellOnEdge 0x4AA440 and the plane's and infantry's
    virtuals are recorded stubs; GetEdge 0x50DA80 and CargoClass::AddPassenger
    0x4733A0 run natively. `infantry` lists each infantry constructor's
    answer (an object or NULL)."""
    infantry = list(infantry if infantry is not None else [True] * max(num, 0))
    emu = Emu()
    emu.write32(AIRCRAFT_TYPE_ITEMS, PD_PLANE_TYPE_ITEMS)
    emu.write32(PD_PLANE_TYPE_ITEMS + 4 * PDPLANE_INDEX, PD_PLANE_TYPE)
    emu.write32(PD_PLANE_TYPE, PD_PLANE_TYPE_VT)
    emu.write32(PD_PLANE_TYPE_VT + 0x8C, STUB_PD_PLANE_CREATE)
    emu.write32(INFANTRY_TYPE_ITEMS, PD_INF_TYPE_ITEMS)
    if inf_type:
        emu.write32(PD_INF_TYPE_ITEMS + 4 * PD_INF_INDEX, PD_INF_TYPES)
    emu.write32(PD_INF_TYPES, PD_INF_TYPE_VT)
    emu.write32(PD_INF_TYPE_VT + 0x8C, STUB_PD_INF_CREATE)
    emu.write32(HOUSE + 0x1E0, edge)
    emu.write32(HOUSE + 0x577C, waypoint_edge)
    for slot, stub in ((0x1E8, STUB_PD_QUEUE), (0x480, STUB_PD_DESTINATION),
                       (0x3C8, STUB_PD_TARGET), (0xD8, STUB_PD_UNLIMBO),
                       (0x2C, STUB_PD_WHAT_AM_I), (0x1EC, STUB_PD_COMMENCE),
                       (0x20, STUB_PD_DELETE)):
        emu.write32(PD_PLANE_VT + slot, stub)
    emu.write32(PD_INFANTRY_VT + 0xD4, STUB_PD_LIMBO)
    plane = pd_plane(0)
    made = []

    def create(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != PD_PLANE_TYPE or e.arg(0) != HOUSE:
            raise OracleError('CreateObject on another type or house')
        e.events.append(['create', e.read_i32(SCENARIO_INIT), created])
        if not created:
            return 0
        e.write32(plane, PD_PLANE_VT)
        return plane

    def create_infantry(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != PD_INF_TYPES or e.arg(0) != HOUSE:
            raise OracleError('CreateObject on another type or house')
        answer = infantry.pop(0)
        e.events.append(['create_infantry', e.read_i32(SCENARIO_INIT), answer])
        if not answer:
            return 0
        this = pd_infantry(len(made))
        made.append(this)
        e.write32(this, PD_INFANTRY_VT)
        write8(e, this + 0x14, ABSTRACT_FOOT)
        return this

    def pick_stub(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != MAP:
            raise OracleError('PickCellOnEdge on another map')
        e.events.append(['pick_cell_on_edge', i32(e.arg(1)), hex(e.arg(2)), hex(e.arg(3)),
                         e.arg(4), e.arg(5) & 0xFF, e.arg(6) & 0xFF, read8(e, plane + 0x3D4)])
        e.uc.mem_write(e.arg(0), struct.pack('<hh', *pick))
        return e.arg(0)

    def target_name(address):
        return {0: 'null', PD_TARGET: 'target'}.get(address, hex(address))

    def unlimbo_stub(e):
        e.events.append(['unlimbo', read_coord(e, e.arg(0)), e.arg(1), e.read_i32(SCENARIO_INIT)])
        return int(unlimbo)

    def limbo(e):
        this = e.uc.reg_read(UC_X86_REG_ECX)
        e.events.append(['limbo', made.index(this), cargo(e, plane)])
        return 0

    emu.hook(STUB_PD_PLANE_CREATE, create, 4)
    emu.hook(STUB_PD_INF_CREATE, create_infantry, 4)
    emu.hook(0x4AA440, pick_stub, 0x1C)
    emu.hook(STUB_PD_QUEUE, lambda e: e.events.append(['queue', e.arg(0), e.arg(1)]), 8)
    emu.hook(STUB_PD_DESTINATION, lambda e: e.events.append(
        ['destination', target_name(e.arg(0)), e.arg(1) & 0xFF]), 8)
    emu.hook(STUB_PD_TARGET, lambda e: e.events.append(['target', target_name(e.arg(0))]), 4)
    emu.hook(STUB_PD_UNLIMBO, unlimbo_stub, 8)
    emu.hook(STUB_PD_WHAT_AM_I, lambda e: e.events.append(['what_am_i']) or what_am_i, 0)
    emu.hook(STUB_PD_COMMENCE, lambda e: e.events.append(['commence', cargo(e, plane)]), 0)
    emu.hook(STUB_PD_DELETE, lambda e: e.events.append(['delete', e.arg(0) & 0xFF]), 4)
    emu.hook(STUB_PD_LIMBO, limbo, 0)
    sent = emu.invoke(0x65E660, ecx=HOUSE, edx=PDPLANE_INDEX,
                      args=[1, 0x1A, PD_TARGET, 0, inf_index, num])
    return dict(inf_index=inf_index, num=num, edge=edge, waypoint_edge=waypoint_edge,
                created=created, unlimbo=unlimbo, what_am_i=what_am_i, inf_type=inf_type,
                pick=list(pick), returned=i32(sent), events=emu.events,
                mission_only=read8(emu, plane + 0x3D4), payload=read8(emu, plane + 0x6C9),
                cargo=cargo(emu, plane), scenario_init=emu.read_i32(SCENARIO_INIT))


def send_paradrop_planes():
    rows = [send_paradrop_planes_row()]
    # A negative num counts down through zero: 2^32 - |num| constructions.
    for num in (0, 1, 9):
        rows.append(send_paradrop_planes_row(num=num))
    rows += [send_paradrop_planes_row(inf_index=-1),
             send_paradrop_planes_row(inf_type=False),
             send_paradrop_planes_row(created=False),
             send_paradrop_planes_row(unlimbo=False),
             send_paradrop_planes_row(what_am_i=1),
             send_paradrop_planes_row(num=3, infantry=[True, False, True]),
             send_paradrop_planes_row(edge=2),
             send_paradrop_planes_row(edge=4, waypoint_edge=3),
             send_paradrop_planes_row(pick=(70, 45))]
    rows += [send_paradrop_planes_row(waypoint_edge=edge) for edge in (1, 2, 3, -1)]
    return rows


# ---------------------------------------------------------- paradrop_missions

def paradrop_mission_row(*, mission, target=True, nav_com=True, passengers=True, distance=0,
                         radius=PD_RADIUS, passes=5, latch=0, in_playfield=True):
    """Mission_ParadropApproach 0x4158E0 (mission 26) or
    Mission_ParadropOverfly 0x415960 (27) on a fixture plane at PD_LOCATION:
    Target +0x2B4, NavCom +0x5A4, first passenger +0x118, the pass counter
    +0x6D3 and the latch +0x6D2. Distance_To 0x5F6440, IsCoordInPlayfield
    0x5785F0, Drop_Payload 0x415C60 and the plane's virtuals are recorded
    stubs."""
    emu = Emu()
    plane = pd_plane(0)
    emu.write32(plane, PD_PLANE_VT)
    emu.write32(plane + 0x2B4, PD_TARGET if target else 0)
    emu.write32(plane + 0x5A4, PD_NAV_COM if nav_com else 0)
    emu.write32(plane + 0x118, pd_infantry(0) if passengers else 0)
    write8(emu, plane + 0x6D3, passes)
    write8(emu, plane + 0x6D2, latch)
    write_coord(emu, plane + 0x9C, PD_LOCATION)
    emu.write32(RULES + 0x54C, radius)
    for slot, stub in ((0x480, STUB_PD_DESTINATION), (0x1E8, STUB_PD_QUEUE),
                       (0x3C8, STUB_PD_TARGET)):
        emu.write32(PD_PLANE_VT + slot, stub)

    def target_name(address):
        return {0: 'null', PD_TARGET: 'target'}.get(address, hex(address))

    def distance_to(e):
        e.events.append(['distance_to', target_name(e.arg(0))])
        return distance

    def playfield(e):
        e.events.append(['in_playfield', read_coord(e, e.arg(0))])
        return int(in_playfield)

    emu.hook(0x5F6440, distance_to, 4)
    emu.hook(0x5785F0, playfield, 4)
    emu.hook(0x415C60, lambda e: e.events.append(['drop_payload', read8(e, plane + 0x6D2)]), 0)
    emu.hook(STUB_PD_DESTINATION, lambda e: e.events.append(
        ['destination', target_name(e.arg(0)), e.arg(1) & 0xFF]), 8)
    emu.hook(STUB_PD_QUEUE, lambda e: e.events.append(['queue', e.arg(0), e.arg(1)]), 8)
    emu.hook(STUB_PD_TARGET, lambda e: e.events.append(['target', target_name(e.arg(0))]), 4)
    entry = {'approach': 0x4158E0, 'overfly': 0x415960}[mission]
    frames = emu.invoke(entry, ecx=plane)
    return dict(mission=mission, target=target, nav_com=nav_com, passengers=passengers,
                distance=distance, radius=radius, passes=passes, latch=latch,
                in_playfield=in_playfield, returned=i32(frames), events=emu.events,
                passes_after=struct.unpack('<b', bytes([read8(emu, plane + 0x6D3)]))[0],
                latch_after=read8(emu, plane + 0x6D2))


def paradrop_missions():
    rows = []
    for mission in ('approach', 'overfly'):
        def row(**kwargs):
            rows.append(paradrop_mission_row(mission=mission, **kwargs))
        for distance in (0, 1, 1023, 1024, 1025, 5000):
            row(distance=distance)
        row(distance=1024, radius=0)
        row(distance=-1, radius=0)
        for passes in (1, 0, -1, -128, 127):
            row(distance=500, passes=passes)
            row(distance=2000, passes=passes)
        row(target=False)
        row(target=False, nav_com=False)
        row(nav_com=False, distance=500)
        row(nav_com=False, distance=2000)
        row(passengers=False, distance=500)
        row(passengers=False, distance=2000, passes=0)
        row(target=False, passengers=False)
        row(distance=500, in_playfield=False)
        row(distance=500, latch=1)
        row(distance=2000, latch=1)
    return rows


# --------------------------------------------------------------- drop_payload

PD_CHUTE_SOUND = 9


def drop_payload_row(*, passengers=2, ammo=100, facing=0x4000, can_enter=0, spot=True,
                     spawned=True, team=False, passes=2, location=PD_LOCATION):
    """AircraftClass::Drop_Payload 0x415C60 on a fixture plane carrying
    `passengers` infantry (CargoClass at +0x114, chained through +0x30;
    RemoveFirstPassenger 0x473430 and AddPassenger 0x4733A0 run natively),
    Ammo +0x2FC and pass counter +0x6D3. GetCoords (vt+0x48) answers
    `location`; FacingClass::Current 0x4C93D0 answers `facing`; the table
    trigonometry 0x4CACB0/0x4CAD00 and ftol run natively. GetCellAt 0x565730
    answers one fixture cell; the passenger's Can_Enter_Cell (vt+0x1AC),
    CellClass 0x481180, SpawnParachuted (vt+0xE8), Limbo (vt+0xD4) and
    vt+0x11C, PlayAt 0x7509E0, the cell's GetCoords and Remove_Member
    0x6EA870 are recorded stubs."""
    emu = Emu()
    plane = pd_plane(0)
    emu.write32(plane, PD_PLANE_VT)
    emu.write32(PD_PLANE_VT + 0x48, STUB_PD_PLANE_COORDS)
    emu.write32(plane + 0x2FC, ammo)
    write8(emu, plane + 0x6D3, passes)
    write_coord(emu, plane + 0x2EC, [-7, -7, -7])
    write_coord(emu, plane + 0x9C, location)
    emu.write32(plane + 0x5D4, PD_TEAM if team else 0)
    emu.write32(RULES + 0x71C, PD_CHUTE_SOUND)
    emu.write32(FRAME, 4321)
    emu.write32(plane + 0x114, passengers)
    for index in range(passengers):
        this = pd_infantry(index)
        emu.write32(this, PD_INFANTRY_VT)
        write8(emu, this + 0x14, ABSTRACT_FOOT)
        emu.uc.mem_write(this + 0x55C, struct.pack('<hh', -3, -3))
        emu.write32(this + 0x30, pd_infantry(index + 1) if index + 1 < passengers else 0)
    emu.write32(plane + 0x118, pd_infantry(0) if passengers else 0)
    for slot, stub in ((0x1AC, STUB_PD_CAN_ENTER), (0xE8, STUB_PD_SPAWN_PARACHUTED),
                       (0xD4, STUB_PD_LIMBO), (0x11C, STUB_PD_CONCEAL)):
        emu.write32(PD_INFANTRY_VT + slot, stub)
    emu.write32(PD_DROP_CELL, PD_DROP_CELL_VT)
    emu.write32(PD_DROP_CELL_VT + 0x48, STUB_PD_CELL_COORDS)

    def passenger(e):
        return (e.uc.reg_read(UC_X86_REG_ECX) - PD_INFANTRY) // PD_INFANTRY_STRIDE

    def facing_stub(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != plane + 0x388:
            raise OracleError('FacingClass::Current on another facing')
        e.uc.mem_write(e.arg(0), struct.pack('<H', facing))
        return e.arg(0)

    def cell_at(e):
        e.events.append(['cell_at', read_coord(e, e.arg(0))])
        return PD_DROP_CELL

    def can_enter_stub(e):
        e.events.append(['can_enter', passenger(e), e.arg(0) == PD_DROP_CELL,
                         [i32(e.arg(n)) for n in range(1, 5)], i32(e.read32(plane + 0x2FC))])
        return can_enter

    def subposition(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != PD_DROP_CELL:
            raise OracleError('0x481180 on another cell')
        e.events.append(['subposition', read_coord(e, e.arg(1)),
                         [e.arg(n) & 0xFF for n in range(2, 5)]])
        x, y, _ = read_coord(e, e.arg(1))
        write_coord(e, e.arg(0), [x + 21, y - 13, 7] if spot else [0, 0, 0])
        return e.arg(0)

    def spawn(e):
        e.events.append(['spawn_parachuted', passenger(e), read_coord(e, e.arg(0))])
        return int(spawned)

    def cell_coords(e):
        out = e.arg(0)
        write_coord(e, out, [40 * 256 + 128, 41 * 256 + 128, 208])
        return out

    emu.hook(STUB_PD_PLANE_COORDS, coords_stub(lambda: read_coord(emu, plane + 0x9C)), 4)
    emu.hook(0x4C93D0, facing_stub, 4)
    emu.hook(0x565730, cell_at, 4)
    emu.hook(STUB_PD_CAN_ENTER, can_enter_stub, 0x14)
    emu.hook(0x481180, subposition, 0x14)
    emu.hook(STUB_PD_SPAWN_PARACHUTED, spawn, 4)
    emu.hook(STUB_PD_CELL_COORDS, cell_coords, 4)
    emu.hook(STUB_PD_LIMBO, lambda e: e.events.append(['limbo', passenger(e)]), 0)
    emu.hook(STUB_PD_CONCEAL, lambda e: e.events.append(['conceal', passenger(e)]), 0)
    emu.hook(0x7509E0, lambda e: e.events.append(
        ['play_at', i32(e.uc.reg_read(UC_X86_REG_ECX)),
         read_coord(e, e.uc.reg_read(UC_X86_REG_EDX)), e.arg(0)]), 4)
    emu.hook(0x6EA870, lambda e: e.events.append(
        ['remove_member', e.uc.reg_read(UC_X86_REG_ECX) == PD_TEAM,
         (e.arg(0) - PD_INFANTRY) // PD_INFANTRY_STRIDE, i32(e.arg(1)), e.arg(2) & 0xFF]), 0xC)
    result = emu.invoke(0x415C60, ecx=plane)
    return dict(passengers=passengers, ammo=ammo, facing=facing, can_enter=can_enter,
                spot=spot, spawned=spawned, team=team, passes=passes, location=list(location),
                returned=i32(result), events=emu.events,
                ammo_after=emu.read_i32(plane + 0x2FC),
                passes_after=struct.unpack('<b', bytes([read8(emu, plane + 0x6D3)]))[0],
                rearm_timer=[emu.read_i32(plane + 0x2EC), emu.read_i32(plane + 0x2F4)],
                cargo=cargo(emu, plane),
                neighbour_cells=[read_cell(emu, pd_infantry(index) + 0x55C)
                                 for index in range(passengers)])


def drop_payload():
    rows = [drop_payload_row(passengers=0)]
    for ammo in (100, 99, 0, -1, 1):
        rows.append(drop_payload_row(ammo=ammo))
    for facing in (0, 0x2000, 0x3FFF, 0xC001, 0xFFFF):
        rows.append(drop_payload_row(facing=facing))
        rows.append(drop_payload_row(facing=facing, ammo=99))
    for can_enter in (1, 2, 7, -1):
        rows.append(drop_payload_row(can_enter=can_enter))
    rows += [drop_payload_row(spot=False), drop_payload_row(spawned=False),
             drop_payload_row(team=True), drop_payload_row(team=True, spawned=False),
             drop_payload_row(passengers=1), drop_payload_row(passengers=1, spawned=False),
             drop_payload_row(passes=0), drop_payload_row(passes=-3, spot=False),
             drop_payload_row(location=(0, 0, 1500)),
             drop_payload_row(location=(-300, 70000, 0))]
    return rows


# ----------------------------------------------------------- spawn_parachuted

def spawn_parachuted_row(*, paradropped=True, human=False, player_control=False, game_mode=0):
    """InfantryClass::SpawnParachuted 0x521760 (vt+0xE8) on a fixture infantry
    owned by HOUSE: ObjectClass::Paradrop 0x5F5940, Queue_Mission (vt+0x1E8)
    and Do_Action (vt+0x558) are recorded stubs; IsControlledByHuman 0x50B730
    runs natively over +0x1EC, +0x1ED and the game mode 0xA8B238."""
    emu = Emu()
    infantry = pd_infantry(0)
    emu.write32(infantry, PD_INFANTRY_VT)
    emu.write32(infantry + 0x21C, HOUSE)
    write8(emu, HOUSE + 0x1EC, human)
    write8(emu, HOUSE + 0x1ED, player_control)
    emu.write32(GAME_MODE, game_mode)
    emu.write32(PD_INFANTRY_VT + 0x1E8, STUB_PD_QUEUE)
    emu.write32(PD_INFANTRY_VT + 0x558, STUB_PD_DO_ACTION)
    write_coord(emu, PD_COORDS, [10261, 10227, 1500])
    emu.hook(0x5F5940, lambda e: e.events.append(
        ['paradrop', e.uc.reg_read(UC_X86_REG_ECX) == infantry, read_coord(e, e.arg(0))])
        or int(paradropped), 4)
    emu.hook(STUB_PD_QUEUE, lambda e: e.events.append(['queue', e.arg(0), e.arg(1)]), 8)
    emu.hook(STUB_PD_DO_ACTION, lambda e: e.events.append(
        ['do_action', i32(e.arg(0)), e.arg(1) & 0xFF, i32(e.arg(2))]), 0xC)
    result = emu.invoke(0x521760, ecx=infantry, args=[PD_COORDS])
    return dict(paradropped=paradropped, human=human, player_control=player_control,
                game_mode=game_mode, returned=i32(result) & 0xFF, events=emu.events)


def spawn_parachuted():
    rows = [spawn_parachuted_row(paradropped=False)]
    for game_mode in (0, 1, 4):
        for human in (False, True):
            for player_control in (False, True):
                rows.append(spawn_parachuted_row(human=human, player_control=player_control,
                                                 game_mode=game_mode))
    return rows

# ------------------------------------------------------------ genetic_launch

TYPE_GENETIC_CONVERTER = 9
GENETIC = BASE + 0x300000
GM_CELLS = GENETIC
GM_CELL_STRIDE = 0x200
GM_CELL_VT = GENETIC + 0x4000
GM_OBJECTS = GENETIC + 0x5000
GM_OBJECT_STRIDE = 0x300
GM_OBJECT_VT = GENETIC + 0xB000
GM_HOUSE = GENETIC + 0xC000
GM_ANIM_TYPE = GENETIC + 0xD000
GM_CELL_ARG = GENETIC + 0xE000
GM_TYPES = GENETIC + 0xF000
GM_TYPE_STRIDE = 0x100
GM_WARHEAD = GENETIC + 0x12000
GM_EXPLOSION_WARHEAD = GENETIC + 0x13000
STUB_GM_CELL_COORDS = STUBS + 0xE40
STUB_GM_WHAT = STUBS + 0xE50
STUB_GM_TYPE = STUBS + 0xE60
STUB_GM_DAMAGE = STUBS + 0xE70

GM_TARGET = (40, 40)
GM_SOUND = 77
WHAT_UNIT = 1
WHAT_BUILDING = 6
WHAT_INFANTRY = 0xF


def gm_object(index):
    return GM_OBJECTS + GM_OBJECT_STRIDE * index


def gm_obj(offset=(0, 0), *, bridge=False, what=WHAT_INFANTRY, strength=125, unlink=False,
           drop_next=False):
    """An object in the target cell offset by `offset`, on its bridge (+0xE8)
    or ground (+0xE4) list, whose WhatAmI (vt+0x2C) answers `what` and whose
    type (vt+0x84) holds Strength `strength` (+0xA0). Inside its
    ReceiveDamage, `unlink` clears its own +0x30 and `drop_next` takes the
    object after it off the list (that one's +0x30 cleared, this one's
    +0x30 moved past it)."""
    return dict(offset=list(offset), bridge=bridge, what=what, strength=strength,
                unlink=unlink, drop_next=drop_next)


def genetic_launch_row(*, objects=(), target=GM_TARGET, bridges=(), levels=None, charged=True,
                       player=False, mute=False, explosion=True):
    """Launch 0x6CC390 from its entry for a type whose Type= (+0xB4) is 9:
    case 9 (0x6CD7E7..0x6CDA62) and the shared tail 0x6CD51E.
    MapClass::operator[] 0x5657A0 answers one fixture cell per coordinate:
    its GetCoords (vt+0x48) answers the centre raised 104 leptons per level
    (`levels`, by offset), its +0x140 carries the bridge bit for the offsets
    in `bridges`, and its ground (+0xE4) and bridge (+0xE8) lists link the
    row's objects, in row order, through +0x30. `explosion` is
    MutateExplosion= (Rules+0x17C8). The anim constructor, PlayEVA,
    PlayAtCoord 0x750E20, CreateRadarEvent, Apply_area_damage 0x489280, each
    object's WhatAmI, GetTechnoType and ReceiveDamage (vt+0x16C) and the EVA
    queue calls are recorded stubs."""
    levels = levels or {}
    emu = Emu()
    emu.invoke(0x6CAE00)
    emu.write32(0xB0C07C, BRIDGE_HEIGHT)
    emu.write32(SUPER + 0x28, SW_TYPE)
    emu.write32(SUPER + 0x2C, GM_HOUSE)
    write8(emu, SUPER + 0x6F, charged)
    emu.write32(SW_TYPE + 0xB4, TYPE_GENETIC_CONVERTER)
    emu.write32(RULES + 0x298, GM_ANIM_TYPE)
    emu.write32(RULES + 0x250, GM_SOUND)
    write8(emu, RULES + 0x17C8, explosion)
    emu.write32(RULES + 0xF98, GM_WARHEAD)
    emu.write32(RULES + 0xF9C, GM_EXPLOSION_WARHEAD)
    emu.write32(SELECTED_SUPER, 9)
    emu.write32(MUTE_LAUNCHES, int(mute))
    emu.uc.mem_write(GM_CELL_ARG, struct.pack('<hh', *target))
    cells = {}

    def cell_at(looked):
        offset = (looked[0] - target[0], looked[1] - target[1])
        if offset not in cells:
            this = GM_CELLS + GM_CELL_STRIDE * len(cells)
            emu.write32(this, GM_CELL_VT)
            emu.write32(this + 0x140, 0x100 if offset in bridges else 0)
            cells[offset] = this
        return cells[offset]

    heads = {}
    for index, spec in enumerate(objects):
        this = gm_object(index)
        emu.write32(this, GM_OBJECT_VT)
        kind = GM_TYPES + GM_TYPE_STRIDE * index
        emu.write32(kind + 0xA0, spec['strength'])
        key = (tuple(spec['offset']), spec['bridge'])
        if key in heads:
            emu.write32(heads[key][-1] + 0x30, this)
            heads[key].append(this)
        else:
            heads[key] = [this]
    for (offset, bridge), listed in heads.items():
        cell = cell_at((target[0] + offset[0], target[1] + offset[1]))
        emu.write32(cell + (0xE8 if bridge else 0xE4), listed[0])

    def index_of(e):
        return (e.uc.reg_read(UC_X86_REG_ECX) - GM_OBJECTS) // GM_OBJECT_STRIDE

    def lookup(e):
        looked = read_cell(e, e.arg(0))
        e.events.append(['cell', looked])
        return cell_at(looked)

    def coords(e):
        out = e.arg(0)
        this = e.uc.reg_read(UC_X86_REG_ECX)
        offset = next(key for key, value in cells.items() if value == this)
        x, y = target[0] + offset[0], target[1] + offset[1]
        write_coord(e, out, [x * 256 + 128, y * 256 + 128,
                             levels.get(offset, 0) * LEVEL_LEPTONS])
        return out

    def what(e):
        index = index_of(e)
        e.events.append(['what', index])
        return objects[index]['what']

    def techno_type(e):
        index = index_of(e)
        e.events.append(['type', index])
        return GM_TYPES + GM_TYPE_STRIDE * index

    def damage(e):
        index = index_of(e)
        this = gm_object(index)
        e.events.append(['damage', index, e.read_i32(e.arg(0)), i32(e.arg(1)),
                         e.arg(2) == GM_WARHEAD, i32(e.arg(3)), e.arg(4) & 0xFF,
                         e.arg(5) & 0xFF, e.arg(6) == GM_HOUSE])
        if objects[index]['unlink']:
            e.write32(this + 0x30, 0)
        if objects[index]['drop_next']:
            after = e.read32(this + 0x30)
            if after:
                e.write32(this + 0x30, e.read32(after + 0x30))
                e.write32(after + 0x30, 0)
        return 0

    def anim(e):
        e.events.append(['anim', e.arg(0) == GM_ANIM_TYPE, read_coord(e, e.arg(1)),
                         [i32(e.arg(n)) for n in range(2, 7)]])
        return e.uc.reg_read(UC_X86_REG_ECX)

    def area_damage(e):
        e.events.append(['area_damage', read_coord(e, e.uc.reg_read(UC_X86_REG_ECX)),
                         i32(e.uc.reg_read(UC_X86_REG_EDX)), i32(e.arg(0)),
                         e.arg(1) == GM_EXPLOSION_WARHEAD, e.arg(2) & 0xFF,
                         e.arg(3) == GM_HOUSE])
        return 0

    def vox_find(e):
        e.events.append(['vox_find', read_name(e, e.uc.reg_read(UC_X86_REG_ECX))])
        return 33

    emu.write32(GM_CELL_VT + 0x48, STUB_GM_CELL_COORDS)
    for slot, stub in ((0x2C, STUB_GM_WHAT), (0x84, STUB_GM_TYPE), (0x16C, STUB_GM_DAMAGE)):
        emu.write32(GM_OBJECT_VT + slot, stub)
    emu.hook(0x5657A0, lookup, 4)
    emu.hook(STUB_GM_CELL_COORDS, coords, 4)
    emu.hook(STUB_GM_WHAT, what, 0)
    emu.hook(STUB_GM_TYPE, techno_type, 0)
    emu.hook(STUB_GM_DAMAGE, damage, 0x1C)
    emu.hook(0x421EA0, anim, 0x1C)
    emu.hook(0x752700, lambda e: e.events.append(
        ['eva', read_name(e, e.uc.reg_read(UC_X86_REG_ECX)), i32(e.uc.reg_read(UC_X86_REG_EDX)),
         i32(e.arg(0))]), 4)
    emu.hook(0x750E20, lambda e: e.events.append(
        ['play_at', i32(e.uc.reg_read(UC_X86_REG_ECX)),
         read_coord(e, e.uc.reg_read(UC_X86_REG_EDX)), i32(e.arg(0))]), 4)
    emu.hook(0x65FA70, lambda e: e.events.append(
        ['radar_event', i32(e.uc.reg_read(UC_X86_REG_ECX)),
         list(struct.unpack('<hh', struct.pack('<I', e.arg(0))))]), 4)
    emu.hook(0x489280, area_damage, 0x10)
    emu.hook(0x753250, vox_find, 0)
    emu.hook(0x752A40, lambda e: e.events.append(
        ['vox_remove', e.uc.reg_read(UC_X86_REG_ECX)]), 0)
    emu.invoke(0x6CC390, ecx=SUPER, args=[GM_CELL_ARG, int(player)])
    return dict(target=list(target), objects=list(objects),
                bridges=[list(offset) for offset in bridges],
                levels=[[list(offset), level] for offset, level in levels.items()],
                charged=charged, player=player, mute=mute, explosion=explosion,
                events=emu.events, selected_super=emu.read_i32(SELECTED_SUPER))


def genetic_launch():
    rows = []
    # MutateExplosion=yes: one area damage at the cell's (deck) coordinate.
    for player in (False, True):
        rows.append(genetic_launch_row(player=player))
    rows.append(genetic_launch_row(charged=False, player=True))
    rows.append(genetic_launch_row(mute=True))
    rows.append(genetic_launch_row(bridges=((0, 0),)))
    for level in (1, 4):
        rows.append(genetic_launch_row(levels={(0, 0): level},
                                       bridges=((0, 0),) if level == 4 else ()))
    # MutateExplosion=no: the 3x3 walk, each infantryman taking its Strength.
    spread = tuple(gm_obj(offset, strength=100 + n) for n, offset in
                   enumerate(reversed(IC_OFFSETS)))
    rows.append(genetic_launch_row(objects=spread, explosion=False))
    rows.append(genetic_launch_row(objects=spread, explosion=False, player=True))
    rows.append(genetic_launch_row(objects=spread, explosion=False, charged=False))
    rows.append(genetic_launch_row(objects=spread, explosion=False, mute=True))
    # Only an Infantry WhatAmI is damaged; its type is read for each.
    rows.append(genetic_launch_row(explosion=False, objects=(
        gm_obj(what=WHAT_UNIT), gm_obj(strength=0), gm_obj(what=WHAT_BUILDING),
        gm_obj(strength=-5), gm_obj(strength=0x7FFFFFFF))))
    # The next object is read before the call: an object leaving its list
    # inside the call does not end the walk, but one taken off the list
    # after it is still visited, and the list ends there.
    rows.append(genetic_launch_row(explosion=False, objects=(
        gm_obj(), gm_obj(unlink=True), gm_obj(), gm_obj((1, 0)))))
    rows.append(genetic_launch_row(explosion=False, objects=(
        gm_obj(), gm_obj(drop_next=True), gm_obj(), gm_obj(), gm_obj((1, 0)))))
    # A bridge cell walks its deck list only.
    on_both = (gm_obj(), gm_obj(bridge=True), gm_obj((1, 0)), gm_obj((1, 0), bridge=True),
               gm_obj((0, 1), bridge=True))
    rows.append(genetic_launch_row(explosion=False, objects=on_both))
    rows.append(genetic_launch_row(explosion=False, objects=on_both, bridges=((0, 0),)))
    rows.append(genetic_launch_row(explosion=False, objects=on_both,
                                   bridges=((1, 0), (0, 1))))
    # The block wraps each word: a target at the map's edge.
    rows.append(genetic_launch_row(explosion=False, objects=(gm_obj((-1, -1)), gm_obj((1, 1))),
                                   target=(0, 0)))
    return rows


# ----------------------------------------------------- infantry_mutate_death

GM_DEATH = GENETIC + 0x20000
GM_INFANTRY = GM_DEATH
GM_INFANTRY_VT = GM_DEATH + 0x1000
GM_DEATH_CELL = GM_DEATH + 0x2000
GM_DEATH_CELL_VT = GM_DEATH + 0x3000
GM_LISTED = GM_DEATH + 0x4000
GM_LISTED_STRIDE = 0x100
GM_LISTED_VT = GM_DEATH + 0x5000
GM_SOURCE = GM_DEATH + 0x6000
GM_SOURCE_HOUSE = GM_DEATH + 0x7000
GM_ARG_HOUSE = GM_DEATH + 0x8000
GM_MUTATE_ANIM_TYPE = GM_DEATH + 0x9000
GM_ANIM_VT = GM_DEATH + 0xA000
GM_SCHEMES = GM_DEATH + 0xB000
GM_DEATH_FRAME = GM_DEATH + 0xC000
STUB_GM_UNMARK = STUBS + 0xE80
STUB_GM_MARK = STUBS + 0xE90
STUB_GM_INF_COORDS = STUBS + 0xEA0
STUB_GM_LISTED_WHAT = STUBS + 0xEB0
STUB_GM_ANIM_MARK = STUBS + 0xEC0

GROUND_TABLE = 0x89EA40
COLOR_SCHEMES = 0xB054D4
GM_LOCATION = (40 * 256 + 100, 41 * 256 + 60, 208)
GM_LAND = 3


def infantry_mutate_death_row(*, location=GM_LOCATION, listed=(), foot_cost=1.0,
                              on_bridge=False, spot=True, source=True, house=True,
                              source_scheme=4, house_scheme=7):
    """InfantryClass::ReceiveDamage's InfDeath 9 arm 0x5188AE..0x518B2C run
    as a slice (ESI the infantryman, the frame's source argument at
    [ESP+0xE0] and house argument at [ESP+0xEC]) to the Crashable test
    0x5185F1, or to the Die2 call 0x5185DD. MapClass::operator[] 0x5657A0
    and GetCellAt 0x565730 answer one fixture cell, whose ground list
    (+0xE4) holds `listed` (WhatAmI answers), whose LandType (+0xEC) is 3 and
    whose Foot cost in the Ground table 0x89EA40 is `foot_cost`; 0x481180
    answers a spot or the empty coordinate 0xA8F200. Mark/Unmark
    (vt+0xF0/vt+0xF4), GetCoords, the anim constructor and the anim's
    MarkCellOccupancy (vt+0xF0) are recorded stubs; SetOwnerHouse 0x424CA0
    runs natively over the source's house (+0x21C) or the house argument,
    and the remap comes from ColorScheme::Array 0xB054D4."""
    emu = Emu()
    infantry = GM_INFANTRY
    emu.write32(infantry, GM_INFANTRY_VT)
    write_coord(emu, infantry + 0x9C, location)
    write8(emu, infantry + 0x8C, on_bridge)
    for slot, stub in ((0xF4, STUB_GM_UNMARK), (0xF0, STUB_GM_MARK),
                       (0x48, STUB_GM_INF_COORDS)):
        emu.write32(GM_INFANTRY_VT + slot, stub)
    emu.write32(GM_DEATH_CELL, GM_DEATH_CELL_VT)
    emu.write32(GM_DEATH_CELL + 0xEC, GM_LAND)
    emu.uc.mem_write(GROUND_TABLE + GM_LAND * 0x24, struct.pack('<f', foot_cost))
    for index, what in enumerate(listed):
        this = GM_LISTED + GM_LISTED_STRIDE * index
        emu.write32(this, GM_LISTED_VT)
        emu.write32(this + 0x80, what)
        emu.write32(this + 0x30, this + GM_LISTED_STRIDE if index + 1 < len(listed) else 0)
    emu.write32(GM_DEATH_CELL + 0xE4, GM_LISTED if listed else 0)
    emu.write32(GM_LISTED_VT + 0x2C, STUB_GM_LISTED_WHAT)
    emu.write32(RULES + 0xB4, GM_MUTATE_ANIM_TYPE)
    emu.write32(GM_SOURCE + 0x21C, GM_SOURCE_HOUSE)
    emu.write32(GM_SOURCE_HOUSE + 0x16054, source_scheme)
    emu.write32(GM_ARG_HOUSE + 0x16054, house_scheme)
    emu.write32(COLOR_SCHEMES, GM_SCHEMES)
    for index in range(10):
        scheme = GM_SCHEMES + 0x100 + 0x400 * index
        emu.write32(GM_SCHEMES + 4 * index, scheme)
        emu.write32(scheme + 0x30C, 0xC0DE00 + index)
    anims = []

    def unmark(e):
        e.events.append(['unmark', read_coord(e, e.arg(0))])

    def mark(e):
        e.events.append(['mark', read_coord(e, e.arg(0))])

    def lookup(e):
        e.events.append(['cell', read_cell(e, e.arg(0))])
        return GM_DEATH_CELL

    def cell_at(e):
        e.events.append(['cell_at', read_coord(e, e.arg(0))])
        return GM_DEATH_CELL

    def listed_what(e):
        this = e.uc.reg_read(UC_X86_REG_ECX)
        e.events.append(['what', (this - GM_LISTED) // GM_LISTED_STRIDE])
        return e.read32(this + 0x80)

    def subposition(e):
        if e.uc.reg_read(UC_X86_REG_ECX) != GM_DEATH_CELL:
            raise OracleError('0x481180 on another cell')
        e.events.append(['subposition', read_coord(e, e.arg(1)),
                         [e.arg(n) & 0xFF for n in range(2, 5)]])
        x, y, _ = read_coord(e, e.arg(1))
        write_coord(e, e.arg(0), [x + 21, y - 13, 7] if spot else [0, 0, 0])
        return e.arg(0)

    def anim(e):
        this = e.uc.reg_read(UC_X86_REG_ECX)
        e.events.append(['anim', e.arg(0) == GM_MUTATE_ANIM_TYPE, read_coord(e, e.arg(1)),
                         [i32(e.arg(n)) for n in range(2, 7)]])
        e.write32(this, GM_ANIM_VT)
        write_coord(e, this + 0x9C, read_coord(e, e.arg(1)))
        anims.append(this)
        return this

    def anim_mark(e):
        e.events.append(['anim_mark', e.uc.reg_read(UC_X86_REG_ECX) in anims,
                         read_coord(e, e.arg(0))])

    emu.write32(GM_ANIM_VT + 0xF0, STUB_GM_ANIM_MARK)
    emu.hook(STUB_GM_UNMARK, unmark, 4)
    emu.hook(STUB_GM_MARK, mark, 4)
    emu.hook(STUB_GM_INF_COORDS, coords_stub(lambda: read_coord(emu, infantry + 0x9C)), 4)
    emu.hook(0x5657A0, lookup, 4)
    emu.hook(0x565730, cell_at, 4)
    emu.hook(STUB_GM_LISTED_WHAT, listed_what, 0)
    emu.hook(0x481180, subposition, 0x14)
    emu.hook(0x421EA0, anim, 0x1C)
    emu.hook(STUB_GM_ANIM_MARK, anim_mark, 4)
    uc = emu.uc
    frame = STACK_BASE + STACK_SIZE - 0x2000
    emu.write32(frame + 0xE0, GM_SOURCE if source else 0)
    emu.write32(frame + 0xEC, GM_ARG_HOUSE if house else 0)
    uc.reg_write(UC_X86_REG_ESP, frame)
    uc.reg_write(UC_X86_REG_ESI, infantry)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    end = run_checked(uc, 0x5188AE, (0x5185F1, 0x5185DD), count=100_000)
    action = None
    if end == 0x5185DD:
        sp = uc.reg_read(UC_X86_REG_ESP)
        action = [emu.read_i32(sp), emu.read_i32(sp + 4), emu.read_i32(sp + 8)]
    owner = remap = None
    if anims:
        owner = {0: None, GM_SOURCE_HOUSE: 'source', GM_ARG_HOUSE: 'house'}.get(
            emu.read32(anims[0] + 0x180), hex(emu.read32(anims[0] + 0x180)))
        remap = emu.read32(anims[0] + 0xD4)
    return dict(location=list(location), listed=list(listed), foot_cost=foot_cost,
                on_bridge=on_bridge, spot=spot, source=source, house=house,
                source_scheme=source_scheme, house_scheme=house_scheme,
                mutated=end == 0x5185F1, action=action, anim_owner=owner,
                anim_remap=remap, events=emu.events)


def infantry_mutate_death():
    rows = [infantry_mutate_death_row()]
    # The anim's house: the source's, else the house argument, else none.
    rows += [infantry_mutate_death_row(source=False),
             infantry_mutate_death_row(source=False, house=False),
             infantry_mutate_death_row(house=False)]
    # A Building on the cell's ground list stops it after the spot test;
    # other objects do not.
    for listed in ((WHAT_INFANTRY,), (WHAT_UNIT, WHAT_INFANTRY), (WHAT_BUILDING,),
                   (WHAT_INFANTRY, WHAT_BUILDING, WHAT_UNIT)):
        rows.append(infantry_mutate_death_row(listed=listed))
    # Ground the Foot cannot enter stops it, unless the infantryman is on a
    # bridge; no spot stops it.
    for foot_cost in (0.0, -0.0, 0.5):
        for on_bridge in (False, True):
            rows.append(infantry_mutate_death_row(foot_cost=foot_cost, on_bridge=on_bridge))
    rows.append(infantry_mutate_death_row(spot=False))
    rows.append(infantry_mutate_death_row(spot=False, foot_cost=0.0))
    rows.append(infantry_mutate_death_row(spot=False, listed=(WHAT_BUILDING,)))
    # The cell from the coordinate's leptons, at a negative coordinate.
    rows.append(infantry_mutate_death_row(location=(-100, 300, 0)))
    rows.append(infantry_mutate_death_row(location=(40 * 256 + 255, 40 * 256, 624)))
    return rows


# -------------------------------------------------------------- make_infantry

GM_MAKE = GENETIC + 0x40000
GM_MAKE_ANIM = GM_MAKE
GM_MAKE_ANIM_VT = GM_MAKE + 0x1000
GM_MAKE_ANIM_TYPE = GM_MAKE + 0x2000
GM_MAKE_HOUSES = GM_MAKE + 0x3000
GM_MAKE_HOUSE_STRIDE = 0x100
GM_MAKE_HOUSE_TYPES = GM_MAKE + 0x4000
GM_MAKE_ITEMS = GM_MAKE + 0x5000
GM_MAKE_INF_TYPES = GM_MAKE + 0x20000
GM_MAKE_INF_TYPE_STRIDE = 0x1000
GM_MAKE_INF_TYPE_VT = GM_MAKE + 0xE000
GM_MAKE_INF_ITEMS = GM_MAKE + 0xF000
GM_MAKE_INFANTRY = GM_MAKE + 0x10000
GM_MAKE_INFANTRY_STRIDE = 0x1000
GM_MAKE_INFANTRY_VT = GM_MAKE + 0x18000
GM_MAKE_CELL = GM_MAKE + 0x19000
GM_MAKE_CELL_VT = GM_MAKE + 0x1A000
GM_MAKE_HOUSE_ITEMS = GM_MAKE + 0x1B000
STUB_GM_ANIM_CLEAR = STUBS + 0xED0
STUB_GM_ANIM_COORDS = STUBS + 0xEE0
STUB_GM_CREATE = STUBS + 0xEF0
STUB_GM_UNLIMBO = STUBS + 0xF00
STUB_GM_MAKE_CELL_COORDS = STUBS + 0xF10
STUB_GM_INF_MARK = STUBS + 0xF20
STUB_GM_QUEUE = STUBS + 0xF30
STUB_GM_ANIM_UNINIT = STUBS + 0xF40

HOUSE_COUNT = 0xA80238
INFANTRY_TYPE_ITEMS = 0xA8E34C
GM_MAKE_LOCATION = (40 * 256 + 100, 41 * 256 + 60, 208)
GM_CIVILIAN_SIDE = 3


def make_infantry_row(*, make_infantry=0, count=1, owner=0, houses=((0, False, False),
                      (GM_CIVILIAN_SIDE, False, False)), civilian_side=GM_CIVILIAN_SIDE,
                      unlimbo=True, bridge=False, location=GM_MAKE_LOCATION, stage=11):
    """AnimClass::AI's MakeInfantry block 0x424932..0x424B30 run as a slice
    on a fixture frame (ESI the anim, EAX its type, EBP -1, EDI 0) to its
    ends. The anim's type holds MakeInfantry (+0x34C); Rules AnimToInfantry
    (+0xCE8, count +0xCF4) lists `count` InfantryTypes whose ArrayIndex
    (+0xDF8) points into InfantryTypeClass::Array 0xA8E34C. HouseClass::Array
    0xA8022C holds `houses`, each (its type's side +0xBC, Defeated +0x1F5,
    IsHuman +0x1EC); `owner` indexes the anim's house (+0x180), None for
    none. SideClass::Find_Index 0x6A46D0 answers `civilian_side` for
    "Civilian". The anim's ClearCellOccupancy (vt+0xF4), GetCoords (vt+0x48)
    and UnInit (vt+0xF8), the type's CreateObject (vt+0x8C), the infantry's
    Unlimbo (vt+0xD8, answering `unlimbo`), Mark (vt+0x124) and Queue_Mission
    (vt+0x1E8) and the cell's GetCoords are recorded stubs; MapClass::
    operator[] 0x5657A0 answers a cell whose +0x140 carries the bridge bit
    when `bridge`."""
    emu = Emu()
    anim = GM_MAKE_ANIM
    emu.write32(anim, GM_MAKE_ANIM_VT)
    emu.write32(anim + 0xC8, GM_MAKE_ANIM_TYPE)
    emu.write32(GM_MAKE_ANIM_TYPE + 0x34C, make_infantry)
    write_coord(emu, anim + 0x9C, location)
    emu.write32(anim + 0xAC, stage)
    for index, (side, defeated, human) in enumerate(houses):
        this = GM_MAKE_HOUSES + GM_MAKE_HOUSE_STRIDE * index
        kind = GM_MAKE_HOUSE_TYPES + 0x100 * index
        emu.write32(this + 0x34, kind)
        emu.write32(kind + 0xBC, side)
        write8(emu, this + 0x1F5, defeated)
        write8(emu, this + 0x1EC, human)
        emu.write32(GM_MAKE_HOUSE_ITEMS + 4 * index, this)
    emu.write32(HOUSE_ITEMS, GM_MAKE_HOUSE_ITEMS)
    emu.write32(HOUSE_COUNT, len(houses))
    emu.write32(anim + 0x180, 0 if owner is None else GM_MAKE_HOUSES + GM_MAKE_HOUSE_STRIDE * owner)
    # AnimToInfantry's items, then one past them (read when MakeInfantry
    # equals the count).
    emu.write32(RULES + 0xCE8, GM_MAKE_ITEMS + 0x40)
    emu.write32(RULES + 0xCF4, count)
    for slot in range(-16, 17):
        kind = GM_MAKE_INF_TYPES + GM_MAKE_INF_TYPE_STRIDE * (slot + 16)
        emu.write32(GM_MAKE_ITEMS + 0x40 + 4 * slot, kind)
        emu.write32(kind + 0xDF8, slot + 16)
        emu.write32(GM_MAKE_INF_ITEMS + 4 * (slot + 16), kind)
        emu.write32(kind, GM_MAKE_INF_TYPE_VT)
    emu.write32(INFANTRY_TYPE_ITEMS, GM_MAKE_INF_ITEMS)
    emu.write32(GM_MAKE_CELL, GM_MAKE_CELL_VT)
    emu.write32(GM_MAKE_CELL + 0x140, 0x100 if bridge else 0)
    for slot, stub in ((0xF4, STUB_GM_ANIM_CLEAR), (0x48, STUB_GM_ANIM_COORDS),
                       (0xF8, STUB_GM_ANIM_UNINIT)):
        emu.write32(GM_MAKE_ANIM_VT + slot, stub)
    emu.write32(GM_MAKE_INF_TYPE_VT + 0x8C, STUB_GM_CREATE)
    for slot, stub in ((0xD8, STUB_GM_UNLIMBO), (0x124, STUB_GM_INF_MARK),
                       (0x1E8, STUB_GM_QUEUE)):
        emu.write32(GM_MAKE_INFANTRY_VT + slot, stub)
    emu.write32(GM_MAKE_CELL_VT + 0x48, STUB_GM_MAKE_CELL_COORDS)
    created = []

    def house_name(address):
        if address == 0:
            return None
        return (address - GM_MAKE_HOUSES) // GM_MAKE_HOUSE_STRIDE

    def find_side(e):
        e.events.append(['find_side', read_name(e, e.uc.reg_read(UC_X86_REG_ECX))])
        return civilian_side

    def create(e):
        kind = e.uc.reg_read(UC_X86_REG_ECX)
        this = GM_MAKE_INFANTRY + GM_MAKE_INFANTRY_STRIDE * len(created)
        e.write32(this, GM_MAKE_INFANTRY_VT)
        created.append(this)
        e.events.append(['create', e.read_i32(kind + 0xDF8) - 16, house_name(e.arg(0))])
        return this

    def unlimbo_stub(e):
        e.events.append(['unlimbo', read_coord(e, e.arg(0)), e.arg(1) & 0xFFFF])
        return int(unlimbo)

    def mark(e):
        this = e.uc.reg_read(UC_X86_REG_ECX)
        e.events.append(['mark', e.arg(0), read8(e, this + 0x8C)])

    def queue(e):
        e.events.append(['queue', i32(e.arg(0)), e.arg(1) & 0xFF])

    def cell_coords(e):
        out = e.arg(0)
        write_coord(e, out, [40 * 256 + 128, 41 * 256 + 128, 208])
        return out

    def lookup(e):
        e.events.append(['cell', read_cell(e, e.arg(0))])
        return GM_MAKE_CELL

    emu.hook(STUB_GM_ANIM_CLEAR, lambda e: e.events.append(
        ['clear', read_coord(e, e.arg(0))]), 4)
    emu.hook(STUB_GM_ANIM_COORDS, coords_stub(lambda: read_coord(emu, anim + 0x9C)), 4)
    emu.hook(STUB_GM_ANIM_UNINIT, lambda e: e.events.append(
        ['uninit', read8(e, anim + 0x179)]), 0)
    emu.hook(0x6A46D0, find_side, 0)
    emu.hook(STUB_GM_CREATE, create, 4)
    emu.hook(STUB_GM_UNLIMBO, unlimbo_stub, 8)
    emu.hook(STUB_GM_INF_MARK, mark, 4)
    emu.hook(STUB_GM_QUEUE, queue, 8)
    emu.hook(STUB_GM_MAKE_CELL_COORDS, cell_coords, 4)
    emu.hook(0x5657A0, lookup, 4)
    uc = emu.uc
    frame = STACK_BASE + STACK_SIZE - 0x2000
    uc.reg_write(UC_X86_REG_ESP, frame)
    uc.reg_write(UC_X86_REG_ESI, anim)
    uc.reg_write(UC_X86_REG_EAX, GM_MAKE_ANIM_TYPE)
    uc.reg_write(UC_X86_REG_EBP, 0xFFFFFFFF)
    uc.reg_write(UC_X86_REG_EDI, 0)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    end = run_checked(uc, 0x424932, (0x424B1B, 0x424B29, 0x424B31), count=100_000)
    return dict(make_infantry=make_infantry, count=count, owner=owner,
                houses=[list(house) for house in houses], civilian_side=civilian_side,
                unlimbo=unlimbo, bridge=bridge, location=list(location), stage=stage,
                end={0x424B1B: 'uninit', 0x424B29: 'retry', 0x424B31: 'plain'}[end],
                anim_owner=house_name(emu.read32(anim + 0x180)),
                stage_after=emu.read_i32(anim + 0xAC), events=emu.events)


def make_infantry():
    rows = [make_infantry_row(make_infantry=-1), make_infantry_row()]
    # MakeInfantry against the list's count: past it, at it (read past the
    # vector) and negative.
    for make, count in ((1, 1), (2, 1), (0, 0), (-2, 1), (3, 4)):
        rows.append(make_infantry_row(make_infantry=make, count=count))
    # The house: none or defeated falls back to the first Civilian-side
    # house; with none such, no house stops it or the defeated one keeps it.
    rows += [make_infantry_row(owner=None),
             make_infantry_row(owner=None, houses=((0, False, False), (1, False, False))),
             make_infantry_row(owner=0, houses=((0, True, False), (GM_CIVILIAN_SIDE, False, False))),
             make_infantry_row(owner=0, houses=((0, True, False), (1, False, False))),
             make_infantry_row(owner=1, houses=((0, False, False), (GM_CIVILIAN_SIDE, True, False),
                                                (GM_CIVILIAN_SIDE, False, False))),
             make_infantry_row(owner=None, houses=()),
             make_infantry_row(owner=None, civilian_side=-1)]
    # Hunt unless the house is human.
    rows.append(make_infantry_row(houses=((0, False, True),)))
    rows.append(make_infantry_row(owner=None, houses=((GM_CIVILIAN_SIDE, False, True),)))
    # Unlimbo refused: the stage steps back and the anim stays.
    rows.append(make_infantry_row(unlimbo=False))
    rows.append(make_infantry_row(unlimbo=False, stage=0))
    # A bridge cell lifts the infantryman onto the deck when the anim is
    # above the cell's ground coordinate.
    for z in (208, 209, 624):
        rows.append(make_infantry_row(bridge=True, location=(40 * 256 + 100, 41 * 256 + 60, z)))
    rows.append(make_infantry_row(bridge=False, location=(40 * 256 + 100, 41 * 256 + 60, 624)))
    rows.append(make_infantry_row(location=(-100, 300, 0)))
    return rows


# ------------------------------------------------------------ psychic_launch

TYPE_PSYCHIC_REVEAL = 11
PSYCHIC = BASE + 0x300000
PR_CELL = PSYCHIC
PR_CELL_VT = PSYCHIC + 0x1000
PR_HOUSE = PSYCHIC + 0x2000
PR_CELL_ARG = PSYCHIC + 0x3000
STUB_PR_CELL_COORDS = STUBS + 0xF50

PR_TARGET = (40, 40)
PR_SOUND = 78


def psychic_launch_row(*, target=PR_TARGET, level=0, bridge=False, radius=15, charged=True,
                       player=False):
    """Launch 0x6CC390 from its entry for a type whose Type= (+0xB4) is 11:
    case 11 (0x6CD70C..0x6CD7E7) and the shared tail 0x6CD51E.
    MapClass::operator[] 0x5657A0 answers one fixture cell: its GetCoords
    (vt+0x48) answers the centre raised 104 leptons per `level`, and its
    +0x140 carries the bridge bit when `bridge`. `radius` is
    PsychicRevealRadius= (Rules+0xFEC). MapClass::RevealArea 0x5678E0,
    PlayAtCoord 0x750E20 and the EVA queue calls are recorded stubs."""
    emu = Emu()
    emu.write32(SUPER + 0x28, SW_TYPE)
    emu.write32(SUPER + 0x2C, PR_HOUSE)
    write8(emu, SUPER + 0x6F, charged)
    emu.write32(SW_TYPE + 0xB4, TYPE_PSYCHIC_REVEAL)
    emu.write32(RULES + 0xFEC, radius)
    emu.write32(RULES + 0x254, PR_SOUND)
    emu.write32(SELECTED_SUPER, 9)
    emu.uc.mem_write(PR_CELL_ARG, struct.pack('<hh', *target))
    emu.write32(PR_CELL, PR_CELL_VT)
    emu.write32(PR_CELL + 0x140, 0x100 if bridge else 0)
    emu.write32(PR_CELL_VT + 0x48, STUB_PR_CELL_COORDS)

    def lookup(e):
        e.events.append(['cell', read_cell(e, e.arg(0))])
        return PR_CELL

    def coords(e):
        out = e.arg(0)
        write_coord(e, out, [target[0] * 256 + 128, target[1] * 256 + 128,
                             level * LEVEL_LEPTONS])
        return out

    def reveal_area(e):
        e.events.append(['reveal_area', e.uc.reg_read(UC_X86_REG_ECX) == MAP,
                         read_coord(e, e.arg(0)), i32(e.arg(1)), e.arg(2) == PR_HOUSE,
                         [i32(e.arg(n)) for n in range(3, 7)], i32(e.arg(7))])
        return 0

    def vox_find(e):
        e.events.append(['vox_find', read_name(e, e.uc.reg_read(UC_X86_REG_ECX))])
        return 33

    emu.hook(0x5657A0, lookup, 4)
    emu.hook(STUB_PR_CELL_COORDS, coords, 4)
    emu.hook(0x5678E0, reveal_area, 0x20)
    emu.hook(0x750E20, lambda e: e.events.append(
        ['play_at', i32(e.uc.reg_read(UC_X86_REG_ECX)),
         read_coord(e, e.uc.reg_read(UC_X86_REG_EDX)), i32(e.arg(0))]), 4)
    emu.hook(0x753250, vox_find, 0)
    emu.hook(0x752A40, lambda e: e.events.append(
        ['vox_remove', e.uc.reg_read(UC_X86_REG_ECX)]), 0)
    emu.invoke(0x6CC390, ecx=SUPER, args=[PR_CELL_ARG, int(player)])
    return dict(target=list(target), level=level, bridge=bridge, radius=radius,
                charged=charged, player=player, events=emu.events,
                selected_super=emu.read_i32(SELECTED_SUPER))


def psychic_launch():
    rows = []
    for charged in (True, False):
        for player in (False, True):
            rows.append(psychic_launch_row(charged=charged, player=player))
    # The coordinate is the cell's GetCoords: its height and no bridge deck.
    for level, bridge in ((1, False), (4, False), (4, True)):
        rows.append(psychic_launch_row(level=level, bridge=bridge))
    # The radius goes to RevealArea as read; RevealArea clamps it
    # (tools/spatial_oracle/reveal_area.py).
    for radius in (0, 3, 11):
        rows.append(psychic_launch_row(radius=radius, player=True))
    rows.append(psychic_launch_row(target=(0, 0)))
    return rows



# --------------------------------------------------------------- nuke_launch

NUKE_LAUNCH = BASE + 0x310000
NL_HOUSE = NUKE_LAUNCH
NL_BUILDING = NUKE_LAUNCH + 0x8000
NL_BUILDING_VT = NUKE_LAUNCH + 0x9000
NL_TYPES = NUKE_LAUNCH + 0xA000
NL_TYPE = NUKE_LAUNCH + 0xB000
NL_TYPE_STRIDE = 0x2000
NL_CELL_ARG = NUKE_LAUNCH + 0x18000
STUB_NL_QUEUE = STUBS + 0xF60
STUB_NL_NEXT = STUBS + 0xF70
BUILDING_TYPE_ITEMS = 0xA83C6C
BUILDING_TYPE_COUNT = 0xA83C78

NL_INDEX = 3
NL_TARGET = (50, 60)
NL_SOUND = 61


def nuke_launch_row(*, types=((True, NL_INDEX, -1),), silo=True, target=NL_TARGET, floor=0,
                    charged=True, player=False, mute=False):
    """Launch 0x6CC390 from its entry for a Type= 0 Super that is not
    one-time (+0x6E clear): case 0 (0x6CDA67, 0x6CDCF0..0x6CDE36).
    BuildingTypeClass::Array (items 0xA83C6C, count 0xA83C78) holds one
    fixture type per `types` entry: (NukeSilo +0x16BA, SuperWeapon +0x16F0,
    SuperWeapon2 +0x16F4) against the Super type's ArrayIndex (+0x98)
    NL_INDEX. Find_Building_Of_Type 0x4FD060 answers the fixture building
    when `silo`, else none; the Map's floor height 0x578080 answers `floor`.
    The building's vt+0x1E8 (Queue_Mission) and vt+0x1EC, PlayAtCoord
    0x750E20, PlayEVA 0x752700 and the EVA queue calls are recorded stubs."""
    emu = Emu()
    emu.write32(SUPER + 0x28, SW_TYPE)
    emu.write32(SUPER + 0x2C, NL_HOUSE)
    write8(emu, SUPER + 0x6F, charged)
    write8(emu, SUPER + 0x6E, 0)
    emu.write32(SW_TYPE + 0xB4, TYPE_MULTI_MISSILE)
    emu.write32(SW_TYPE + 0x98, NL_INDEX)
    emu.write32(RULES + 0x174, NL_SOUND)
    emu.write32(MUTE_LAUNCHES, int(mute))
    emu.write32(SELECTED_SUPER, 9)
    emu.write32(NL_BUILDING, NL_BUILDING_VT)
    emu.write32(NL_BUILDING + 0x5F8, -1)
    emu.write32(NL_BUILDING_VT + 0x1E8, STUB_NL_QUEUE)
    emu.write32(NL_BUILDING_VT + 0x1EC, STUB_NL_NEXT)
    for slot, (nuke_silo, weapon, weapon2) in enumerate(types):
        address = NL_TYPE + slot * NL_TYPE_STRIDE
        emu.write32(NL_TYPES + 4 * slot, address)
        write8(emu, address + 0x16BA, nuke_silo)
        emu.write32(address + 0x16F0, weapon)
        emu.write32(address + 0x16F4, weapon2)
    emu.write32(BUILDING_TYPE_ITEMS, NL_TYPES)
    emu.write32(BUILDING_TYPE_COUNT, len(types))
    emu.uc.mem_write(NL_CELL_ARG, struct.pack('<hh', *target))

    def find(e):
        e.events.append(['find', e.uc.reg_read(UC_X86_REG_ECX) == NL_HOUSE, i32(e.arg(0)),
                         i32(e.arg(1))])
        return NL_BUILDING if silo else 0

    def floor_height(e):
        e.events.append(['floor', e.uc.reg_read(UC_X86_REG_ECX) == MAP, read_coord(e, e.arg(0))])
        return floor

    def vox_find(e):
        e.events.append(['vox_find', read_name(e, e.uc.reg_read(UC_X86_REG_ECX))])
        return 33

    emu.hook(0x4FD060, find, 8)
    emu.hook(0x578080, floor_height, 4)
    emu.hook(STUB_NL_QUEUE, lambda e: e.events.append(
        ['queue_mission', e.uc.reg_read(UC_X86_REG_ECX) == NL_BUILDING, i32(e.arg(0)),
         i32(e.arg(1))]), 8)
    emu.hook(STUB_NL_NEXT, lambda e: e.events.append(
        ['next_mission', e.uc.reg_read(UC_X86_REG_ECX) == NL_BUILDING]), 0)
    emu.hook(0x750E20, lambda e: e.events.append(
        ['play_at', i32(e.uc.reg_read(UC_X86_REG_ECX)),
         read_coord(e, e.uc.reg_read(UC_X86_REG_EDX)), i32(e.arg(0))]), 4)
    emu.hook(0x752700, lambda e: e.events.append(
        ['eva', read_name(e, e.uc.reg_read(UC_X86_REG_ECX)), i32(e.uc.reg_read(UC_X86_REG_EDX)),
         i32(e.arg(0))]), 4)
    emu.hook(0x753250, vox_find, 0)
    emu.hook(0x752A40, lambda e: e.events.append(
        ['vox_remove', e.uc.reg_read(UC_X86_REG_ECX)]), 0)
    emu.invoke(0x6CC390, ecx=SUPER, args=[NL_CELL_ARG, int(player)])
    return dict(types=[list(entry) for entry in types], silo=silo, target=list(target),
                floor=floor, charged=charged, player=player, mute=mute, events=emu.events,
                nuke_target=list(struct.unpack('<hh', emu.uc.mem_read(NL_HOUSE + 0x5784, 4))),
                firing_type=emu.read_i32(NL_BUILDING + 0x5F8),
                recheck=read8(emu, NL_HOUSE + 0x1FC),
                selected_super=emu.read_i32(SELECTED_SUPER))


def nuke_launch():
    rows = []
    for charged in (True, False):
        for player in (False, True):
            rows.append(nuke_launch_row(charged=charged, player=player))
    # No silo standing: the tail and the recheck run all the same.
    for player in (False, True):
        rows.append(nuke_launch_row(silo=False, player=player))
    # The first NukeSilo type whose SuperWeapon or SuperWeapon2 is the
    # Super's type is the one asked for; with none, nothing is asked.
    rows.append(nuke_launch_row(types=((False, NL_INDEX, -1), (True, -1, NL_INDEX))))
    rows.append(nuke_launch_row(types=((True, 1, 2), (True, NL_INDEX, -1), (True, NL_INDEX, -1))))
    rows.append(nuke_launch_row(types=((True, 1, 2), (False, NL_INDEX, NL_INDEX)), player=True))
    rows.append(nuke_launch_row(types=(), player=True))
    # PlayAtCoord's coordinate carries the cell's floor height; the house
    # keeps the cell.
    rows.append(nuke_launch_row(target=(7, 90), floor=416))
    rows.append(nuke_launch_row(mute=True, player=True))
    return rows



# ------------------------------------------------------------ super_timers

TIMERS = BASE + 0x3D0000
TM_SUPERS = TIMERS
TM_SUPER_STRIDE = 0x100
TM_TYPES = TIMERS + 0x2000
TM_TYPE_STRIDE = 0x100
TM_LABELS = TIMERS + 0x4000
TM_LABEL_STRIDE = 0x40
TM_BLACKOUT_LABEL = TM_LABELS + 0x1800
TM_ITEMS = TIMERS + 0x6000
TM_HOUSE_ITEMS = TIMERS + 0x6800
TM_SCHEME_ITEMS = TIMERS + 0x7000
TM_SCHEMES = TIMERS + 0x8000
TM_SCHEME_STRIDE = 0x400
TM_SCHEME_COUNT = 32
TM_FONT = TIMERS + 0x18000
TM_SURFACE = TIMERS + 0x18100
TM_SURFACE_VT = TIMERS + 0x18200
TM_BLINK = TIMERS + 0x18400
TM_HOUSE_SIZE = 0x16100
STUB_TM_RECT = STUBS + 0xF80
STUB_TM_FILL = STUBS + 0xF90

SUPER_TIMER_ITEMS = 0xA83D54
SUPER_TIMER_COUNT = 0xA83D60
HOUSE_ITEMS = 0xA8022C
HOUSE_COUNT = 0xA80238
PLAYER_PTR = 0xA83D4C
SCHEME_ITEMS = 0xB054D4
VIEW_W = 0x886FA8
VIEW_H = 0x886FAC
FONT_PTR = 0x89C4D0
COMPOSITE = 0x88731C
# The pixel format's shifts (right, then left) for HSV_To_RGB's three output
# bytes, set here to RGB565.
PIXEL_SHIFTS = ((0x8A0DD4, 0x8A0DD0, 3, 11), (0x8A0DE4, 0x8A0DE0, 2, 5),
                (0x8A0DDC, 0x8A0DD8, 3, 0))
TM_OWNER_HSV = (153, 214, 212)
TM_BLINK_HSV = (0, 0, 200)


def write_wide(emu, address, text):
    emu.uc.mem_write(address, (text + '\0').encode('utf-16-le'))


def read_wide(emu, address):
    out = bytearray()
    while True:
        pair = bytes(emu.uc.mem_read(address + len(out), 2))
        if pair == b'\0\0':
            return out.decode('utf-16-le')
        out += pair


def timer_glyph_width(text):
    """The stub measure: 4 pixels a space, 6 a digit or colon, 8 anything
    else, and one pixel of spacing after every character, as BitFont's
    measure adds its +0x2C after each one."""
    return sum(4 if c == ' ' else 6 if c in '0123456789:' else 8 for c in text) + len(text)


def tm_house(emu, scheme, blackout):
    address = emu.heap
    emu.heap += TM_HOUSE_SIZE
    emu.write32(address + 0x16054, scheme)
    emu.write32(address + 0x2A4, blackout[0])
    emu.write32(address + 0x2AC, blackout[1])
    return address


def tm_super(owner, start, left, *, hold=False, custom=-1, recharge=9000, name='Nuke'):
    return dict(owner=owner, start=start, left=left, hold=hold, custom=custom,
                recharge=recharge, name=name)


def tactical_timers_row(*, supers=(), houses=((3, (-1, 0)),), scenario=(-1, 0), game_mode=5,
                        frame=1000, player=0):
    """TacticalClass::Draw's timer block (0x6D4941..0x6D4B25) as a slice: the
    Scenario timer (+0x11E8/+0x11F0, label +0x11F4) in the player's scheme
    (PlayerPtr 0xA83D4C, House+0x16054, ColorScheme::Array items 0xB054D4),
    then each Super of the timer list (items 0xA83D54, count 0xA83D60) with
    GameMode 0xA8B238's on-hold arm and GetRechargeTime 0x6CC260 run
    natively, then each house's blackout timer (+0x2A4/+0x2AC, HouseClass::
    Array items 0xA8022C, count 0xA80238) labelled through the string table
    0x734E60. The string table and the line draw 0x6D4B50 are recorded
    stubs. `houses` holds (scheme index, blackout (start, left))."""
    emu = Emu()
    emu.write32(FRAME, frame)
    emu.write32(GAME_MODE, game_mode)
    emu.write32(SCHEME_ITEMS, TM_SCHEME_ITEMS)
    for index in range(TM_SCHEME_COUNT):
        emu.write32(TM_SCHEME_ITEMS + 4 * index, TM_SCHEMES + index * TM_SCHEME_STRIDE)
    house_addresses = [tm_house(emu, scheme, blackout) for scheme, blackout in houses]
    emu.write32(HOUSE_ITEMS, TM_HOUSE_ITEMS)
    for index, address in enumerate(house_addresses):
        emu.write32(TM_HOUSE_ITEMS + 4 * index, address)
    emu.write32(HOUSE_COUNT, len(house_addresses))
    emu.write32(PLAYER_PTR, house_addresses[player])
    emu.write32(SCENARIO + 0x11E8, scenario[0])
    emu.write32(SCENARIO + 0x11F0, scenario[1])
    write_wide(emu, TM_LABELS, 'Mission')
    emu.write32(SCENARIO + 0x11F4, TM_LABELS)
    write_wide(emu, TM_BLACKOUT_LABEL, 'Blackout')
    emu.write32(SUPER_TIMER_ITEMS, TM_ITEMS)
    emu.write32(SUPER_TIMER_COUNT, len(supers))
    for slot, sw in enumerate(supers):
        address = TM_SUPERS + slot * TM_SUPER_STRIDE
        type_address = TM_TYPES + slot * TM_TYPE_STRIDE
        label = TM_LABELS + (slot + 1) * TM_LABEL_STRIDE
        emu.write32(TM_ITEMS + 4 * slot, address)
        emu.write32(address + 0x24, sw['custom'])
        emu.write32(address + 0x28, type_address)
        emu.write32(address + 0x2C, house_addresses[sw['owner']])
        emu.write32(address + 0x30, sw['start'])
        emu.write32(address + 0x38, sw['left'])
        write8(emu, address + 0x70, sw['hold'])
        emu.write32(type_address + 0x60, label)
        emu.write32(type_address + 0xB0, sw['recharge'])
        write_wide(emu, label, sw['name'])

    def string_table(e):
        e.events.append(['load_string', read_name(e, e.uc.reg_read(UC_X86_REG_ECX))])
        return TM_BLACKOUT_LABEL

    def line(e):
        timer, state = e.arg(2), e.arg(3)
        blink = None
        if timer or state:
            if timer != state + 8 or (state - 0x40 - TM_SUPERS) % TM_SUPER_STRIDE:
                raise OracleError('a timer line\'s blink pointers are not a Super\'s')
            blink = (state - 0x40 - TM_SUPERS) // TM_SUPER_STRIDE
        scheme = e.uc.reg_read(UC_X86_REG_EDX)
        e.events.append(['line', i32(e.uc.reg_read(UC_X86_REG_ECX)),
                         (scheme - TM_SCHEMES) // TM_SCHEME_STRIDE, i32(e.arg(0)),
                         read_wide(e, e.arg(1)), blink])

    emu.hook(0x734E60, string_table, 8)
    emu.hook(0x6D4B50, line, 0x10)
    uc = emu.uc
    uc.reg_write(UC_X86_REG_ESP, STACK_BASE + STACK_SIZE - 0x1000)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    run_checked(uc, 0x6D4941, 0x6D4B25, count=100_000)
    return dict(supers=list(supers), houses=[[scheme, list(blackout)] for scheme, blackout in houses],
                scenario=list(scenario), game_mode=game_mode, frame=frame, player=player,
                events=emu.events)


def tactical_timers():
    row = tactical_timers_row
    houses = ((3, (-1, 0)), (11, (-1, 0)), (21, (-1, 0)))
    rows = [row(houses=houses)]
    # The Scenario timer takes line 0 while it runs, the Supers follow in
    # list order, any owner's.
    supers = (tm_super(0, 400, 9000), tm_super(1, -1, 3000, name='Curtain'),
              tm_super(0, 0, 900, name='Storm'))
    for scenario in ((900, 3000), (-1, 500), (0, 500), (100, 54000)):
        rows.append(row(supers=supers, houses=houses, scenario=scenario))
    rows.append(row(supers=supers, houses=houses, player=2, scenario=(1000, 15)))
    # Seconds are frames left over 15, truncated.
    rows.append(row(supers=tuple(tm_super(0, -1, left) for left in (0, 14, 15, 29, 30, 899,
                                                                    900, 53999, 54000)),
                    houses=houses))
    # In GameMode 0 a held Super whose timer has all of GetRechargeTime left
    # takes no line; the next one takes its index.
    held = (tm_super(0, -1, 9000, hold=True), tm_super(1, -1, 8999, hold=True),
            tm_super(0, -1, 9000), tm_super(1, -1, 4500, hold=True, custom=4500),
            tm_super(0, -1, 9000, hold=True, custom=4500), tm_super(2, 1000, 9000, hold=True))
    for game_mode in (0, 5):
        rows.append(row(supers=held, houses=houses, game_mode=game_mode))
    # Each house's blackout timer with time left follows, in house order.
    blackouts = ((3, (900, 300)), (11, (-1, 450)), (21, (0, 100)), (13, (-1, 0)),
                 (25, (1000, 1)), (29, (-1, -15)))
    rows.append(row(supers=supers[:1], houses=blackouts))
    rows.append(row(houses=blackouts, scenario=(990, 30)))
    return rows


def timer_line_row(*, index=0, seconds=600, label='Nuke', blink=None, now=5000,
                   view=(632, 432), height=17):
    """0x6D4B50 for one timer line: Fancy_Text_Print_Wide 0x4A61C0 and its
    print 0x4A5EB0, HSV_To_RGB 0x517440 and the font setters run natively;
    swprintf 0x7CA564 and vswprintf 0x7CA858, the measure 0x433CF0
    (timer_glyph_width), the clock 0x4093B0, the Composite surface's
    GetRect (vt+0x78) and FillRect (vt+0x14) and the glyph print 0x434B90
    are recorded stubs. The line's scheme holds TM_OWNER_HSV and
    ColorScheme::Array[5] TM_BLINK_HSV (+0x308); the pixel format is RGB565.
    `blink` is the Super's (+0x40, +0x48) or None for null pointers."""
    emu = Emu()
    emu.write32(VIEW_W, view[0])
    emu.write32(VIEW_H, view[1])
    emu.write32(FONT_PTR, TM_FONT)
    emu.write32(TM_FONT + 0x1C, height)
    emu.write32(COMPOSITE, TM_SURFACE)
    emu.write32(TM_SURFACE, TM_SURFACE_VT)
    emu.write32(TM_SURFACE_VT + 0x78, STUB_TM_RECT)
    emu.write32(TM_SURFACE_VT + 0x14, STUB_TM_FILL)
    emu.write32(SCHEME_ITEMS, TM_SCHEME_ITEMS)
    for index_ in range(TM_SCHEME_COUNT):
        emu.write32(TM_SCHEME_ITEMS + 4 * index_, TM_SCHEMES + index_ * TM_SCHEME_STRIDE)
    emu.uc.mem_write(TM_SCHEMES + 0x308, bytes(TM_OWNER_HSV))
    emu.uc.mem_write(TM_SCHEMES + 5 * TM_SCHEME_STRIDE + 0x308, bytes(TM_BLINK_HSV))
    for right, left, right_shift, left_shift in PIXEL_SHIFTS:
        write8(emu, right, right_shift)
        emu.write32(left, left_shift)
    write_wide(emu, TM_LABELS, label)
    if blink is not None:
        write8(emu, TM_BLINK, blink[0])
        emu.uc.mem_write(TM_BLINK + 8, struct.pack('<Q', blink[1]))

    def swprintf(e):
        fmt = read_wide(e, e.arg(1))
        out, at, arg = [], 0, 2
        while at < len(fmt):
            if fmt[at] != '%':
                out.append(fmt[at])
                at += 1
                continue
            for spec, render in (('s', lambda v: read_wide(e, v)), ('d', lambda v: str(i32(v))),
                                 ('02d', lambda v: '%02d' % i32(v))):
                if fmt.startswith(spec, at + 1):
                    out.append(render(e.arg(arg)))
                    at += 1 + len(spec)
                    arg += 1
                    break
            else:
                raise OracleError(f'unexpected swprintf format {fmt!r}')
        text = ''.join(out)
        write_wide(e, e.arg(0), text)
        return len(text)

    def vswprintf(e):
        text = read_wide(e, e.arg(1))
        if '%' in text:
            raise OracleError(f'vswprintf format {text!r}')
        write_wide(e, e.arg(0), text)
        return len(text)

    def measure(e):
        text = read_wide(e, e.arg(0))
        if e.arg(1):
            e.write32(e.arg(1), timer_glyph_width(text))
        if e.arg(2):
            e.write32(e.arg(2), e.read_i32(TM_FONT + 0x1C))
        e.events.append(['measure', text, i32(e.arg(3))])

    def clock(e):
        e.uc.reg_write(UC_X86_REG_EDX, now >> 32)
        return now & 0xFFFFFFFF

    def get_rect(e):
        e.uc.mem_write(e.arg(0), struct.pack('<iiii', 0, 0, view[0], view[1]))
        return e.arg(0)

    def fill(e):
        e.events.append(['fill', list(struct.unpack('<iiii', e.uc.mem_read(e.arg(0), 16))),
                         i32(e.arg(1))])
        return 1

    def glyphs(e):
        color = struct.unpack('<H', e.uc.mem_read(TM_FONT + 0x24, 2))[0]
        e.events.append(['print', read_wide(e, e.arg(2)), i32(e.arg(3)), i32(e.arg(4)), color,
                         list(struct.unpack('<iiii', e.uc.mem_read(TM_FONT + 0x30, 16)))])

    emu.hook(0x7CA564, swprintf, 0)
    emu.hook(0x7CA858, vswprintf, 0)
    emu.hook(0x433CF0, measure, 0x10)
    emu.hook(0x4093B0, clock, 0)
    emu.hook(STUB_TM_RECT, get_rect, 4)
    emu.hook(STUB_TM_FILL, fill, 8)
    emu.hook(0x434B90, glyphs, 0x1C)
    pointers = (TM_BLINK + 8, TM_BLINK) if blink is not None else (0, 0)
    emu.invoke(0x6D4B50, ecx=index, edx=TM_SCHEMES, args=[seconds, TM_LABELS, *pointers])
    after = None
    if blink is not None:
        after = [read8(emu, TM_BLINK), struct.unpack('<Q', emu.uc.mem_read(TM_BLINK + 8, 8))[0]]
    return dict(index=index, seconds=seconds, label=label, blink=list(blink) if blink else None,
                now=now, view=list(view), height=height, scheme_hsv=list(TM_OWNER_HSV),
                blink_hsv=list(TM_BLINK_HSV), events=emu.events, blink_after=after)


def timer_lines():
    row = timer_line_row
    rows = [row(seconds=seconds) for seconds in (0, 1, 59, 60, 61, 599, 600, 3599, 3600, 3661,
                                                 35999, 36000, 359999)]
    rows += [row(index=index) for index in (1, 2, 5)]
    rows.append(row(view=(1112, 718), index=3, seconds=125, label='Iron Curtain'))
    rows.append(row(height=12, index=2))
    # At zero seconds a Super's line toggles +0x40 once its +0x48 deadline is
    # not ahead of the clock, the next deadline 1000 ms on; while it is set
    # the time takes ColorScheme::Array[5].
    for blink, now in (((0, 0), 5000), ((1, 6000), 5500), ((1, 6000), 6000),
                       ((0, 5000), 4999), ((0, 5000), 5000), ((1, 1 << 32), (1 << 32) - 1),
                       ((0, 1 << 32), (1 << 32) + 5)):
        rows.append(row(seconds=0, blink=blink, now=now))
    rows.append(row(seconds=5, blink=(1, 0), now=5000))
    return rows


# ---------------------------------------------------------------- IonBlast

ION_VECTOR_INIT = 0x53CA70       # the vector's static initializer (vtable, growth)
ION_ITEMS = 0xAA011C             # DynamicVectorClass 0xAA0118: items, capacity, count
ION_CAPACITY = 0xAA0120
ION_ALLOCATED = 0xAA0125
ION_COUNT = 0xAA0128
ION_RIPPLE_SURFACES = 0xA9FFC8   # 80 BSurface pointers
ION_RIPPLE_STEP = 0xA9FF80       # and its copy 0xA9FFA8
ION_RIPPLE_BUILT = 0xAA014C
ION_RIPPLE_FRAMES = 80
ION_RIPPLE_SIZE = (512, 256)
ION_STEP_INITIALIZERS = (0x53CA30, 0x53CA50)
BSURFACE_VT = 0x7E2070
DETAIL_LEVEL = 0xA8EB78
TEMP_SURFACE = 0x887314
TACTICAL_PTR = 0x887324
ZBUFFER_PTR = 0x887644
TACTICAL_VIEW = 0x886FA0          # offset X, offset Y, width, height
TACTICAL_CACHED_RECT = 0xB0CE28   # CoordsToClient2's view (+8 width, +0xC height)
ADJUST_FOR_Z_SCALE = 0xB0CD48
STANDARD_Z_MULTIPLIER_BITS = 0x3FC25E5374344960  # startup store 0x6D1BDD
ION_HEAP, ION_HEAP_SIZE = 0x70000000, 0x01000000
# Every byte the generator writes (checked against its frames): the fill and
# the spiral indexes of (0, v) for v 0..6.
ION_RIPPLE_BYTES = (0xFF, 0x00, 4, 15, 34, 61, 96, 0x8B)
# The draw fixture: a 192x128 tactical view at the surface's origin, with 16
# rows below it that the draw reads (a byte moves up to ten rows).
ION_VIEW = (192, 128)
ION_SURFACE_ROWS = 144
ION_BASE_COORD = (25600, 25600)


class IonEmu(Emu):
    """Emu whose operator new (and the draw fixture) take a heap with room
    for the 80 ripple surfaces."""

    def __init__(self):
        self.ion_heap = ION_HEAP
        super().__init__()
        self.uc.mem_map(ION_HEAP, ION_HEAP_SIZE)

    def operator_new(self, _emu):
        return self.alloc(self.arg(0))

    def alloc(self, size):
        address = self.ion_heap
        self.ion_heap += (size + 15) & ~15
        if self.ion_heap > ION_HEAP + ION_HEAP_SIZE:
            raise OracleError('IonBlast heap exhausted')
        return address


def fnv1a64_bytes(data):
    digest = 0xCBF29CE484222325
    for byte in data:
        digest = ((digest ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return digest


def ripple_step_bits(fpcw):
    """The ring step 0xA9FF80 and its copy 0xA9FFA8 after both initializers."""
    emu = Emu()
    for entry in ION_STEP_INITIALIZERS:
        invoke_under(emu, entry, fpcw=fpcw)
    return [f'0x{struct.unpack("<Q", emu.uc.mem_read(address, 8))[0]:016x}'
            for address in (ION_RIPPLE_STEP, ION_RIPPLE_STEP + 0x28)]


def ion_blast_ripple():
    """The ring step's initializers under the startup and the process control
    words; the generator 0x53D330 (Show_Loading_Screen's call) under the
    process word after the startup initializers: each frame's FNV-1a over its
    512x256 bytes, its written (not -1) count and its byte values; and the
    spiral functions 0x53D8E0 (index 1..288, DrawAll's table) and 0x53D960
    (x, y in -9..9)."""
    emu = IonEmu()
    for entry in ION_STEP_INITIALIZERS:
        invoke_under(emu, entry, fpcw=STARTUP_FPCW)
    # 116 million instructions under the observing hook.
    invoke_under(emu, 0x53D330, count=400_000_000, timeout_us=1_800_000_000)
    if read8(emu, ION_RIPPLE_BUILT) != 1:
        raise OracleError('the ripple latch is not set')
    width, height = ION_RIPPLE_SIZE
    frames, values = [], set()
    for n in range(ION_RIPPLE_FRAMES):
        surface = emu.read32(ION_RIPPLE_SURFACES + 4 * n)
        if [emu.read32(surface + offset) for offset in (0, 4, 8, 0x10)] != [BSURFACE_VT, width,
                                                                            height, 1]:
            raise OracleError(f'ripple surface {n} is not a 512x256 byte BSurface')
        data = bytes(emu.uc.mem_read(emu.read32(surface + 0x14), width * height))
        frame_values = sorted(set(data))
        values.update(frame_values)
        frames.append(dict(fnv1a64=f'0x{fnv1a64_bytes(data):016x}',
                           written=sum(1 for byte in data if byte != 0xFF),
                           values=frame_values))
    if values != set(ION_RIPPLE_BYTES):
        raise OracleError(f'the generator wrote {sorted(values)}')
    out = emu.alloc(8)
    points = []
    for index in range(1, 289):
        invoke_under(emu, 0x53D8E0, ecx=out, edx=index)
        points.append([index, *struct.unpack('<ii', emu.uc.mem_read(out, 8))])
    indexes = []
    for x in range(-9, 10):
        for y in range(-9, 10):
            emu.invoke(0x53D960, args=[x & 0xFFFFFFFF, y & 0xFFFFFFFF])
            indexes.append([x, y, i32(emu.uc.reg_read(UC_X86_REG_EAX))])
    return dict(step_bits={f'0x{fpcw:04X}': ripple_step_bits(fpcw)
                           for fpcw in (STARTUP_FPCW, NATIVE_FPCW)},
                generator_fpcw=f'0x{NATIVE_FPCW:04X}', frames=frames,
                spiral_points=points, spiral_indexes=indexes)


def ion_vector(emu, blasts):
    """The IonBlast vector after its initializer, holding fixture blasts
    (X, Y, Z, frame, flag); not allocated, so nothing frees its items."""
    invoke_under(emu, ION_VECTOR_INIT)
    items = emu.alloc(4 * max(1, len(blasts)))
    pointers = []
    for index, (x, y, z, frame, flag) in enumerate(blasts):
        blast = emu.alloc(0x14)
        emu.uc.mem_write(blast, struct.pack('<iiiii', x, y, z, frame, flag))
        emu.write32(items + 4 * index, blast)
        pointers.append(blast)
    emu.write32(ION_ITEMS, items)
    emu.write32(ION_CAPACITY, max(1, len(blasts)))
    write8(emu, ION_ALLOCATED, 0)
    emu.write32(ION_COUNT, len(blasts))
    return pointers


def ion_blast_update():
    """IonBlastClass::UpdateAll 0x53D310 over forceless (+0x10 = 1) blasts:
    each blast left in the vector, in order, as its index in `frames` and its
    frame after the update (the vector's Find vt+0x10 and RemoveAt 0x53DDA0
    run natively; operator delete 0x7C8B3D is a silent stub)."""
    rows = []
    for frames in ([0], [78], [79], [80], [0, 5, 78, 79, 80, 3], [79, 79, 1]):
        emu = IonEmu()
        pointers = ion_vector(emu, [(0, 0, 0, frame, 1) for frame in frames])
        invoke_under(emu, 0x53D310)
        items = emu.read32(ION_ITEMS)
        left = [emu.read32(items + 4 * i) for i in range(emu.read_i32(ION_COUNT))]
        rows.append(dict(frames=frames,
                         left=[[pointers.index(p), emu.read_i32(p + 0xC)] for p in left]))
    return rows


def ion_ripple_pattern(n, sx, sy):
    """The draw fixture's ripple byte at (sx, sy) of frame n: every value the
    generator writes, in a pattern that varies by row, column and frame."""
    return ION_RIPPLE_BYTES[(sx * 3 + sy * 5 + n * 7) % len(ION_RIPPLE_BYTES)]


def ion_background_word(x, y):
    return (x * 0x0123 + y * 0x0B57 + 0x1F) & 0xFFFF


def ion_z_word(x, y, z_origin):
    """The fixture Z around a threshold row: z_origin - y, offset -2..2."""
    return (z_origin - y + (x * 7 + y * 3) % 5 - 2) & 0xFFFF


def ion_byte_surface(emu, width, height, bytes_per_pixel, data):
    """A BSurface (vtable 0x7E2070; +4 width, +8 height, +0xC lock count,
    +0x10 bytes per pixel, +0x14 Buffer) over a fixture buffer."""
    surface = emu.alloc(0x20)
    buffer = emu.alloc(len(data))
    emu.uc.mem_write(buffer, bytes(data))
    emu.uc.mem_write(surface, struct.pack('<IIIIIIIB', BSURFACE_VT, width, height, 0,
                                          bytes_per_pixel, buffer, len(data), 0))
    return surface, buffer


def ion_draw_row(*, name, blasts, view_origin, z_origin, detail=2):
    """IonBlastClass::DrawAll 0x53D850 (the offset table it builds, and
    0x53D580 per blast, with CoordsToClient2 0x6D2140, AdjustForZ 0x6D20E0,
    Prep_For_Blit 0x7BC040 and the BSurface methods run natively) on fixture
    surfaces: the destination and the Z buffer are byte BSurfaces of
    ION_VIEW's width and ION_SURFACE_ROWS rows, the ripple frames carry
    ion_ripple_pattern. Blasts are (X, Y, Z, frame)."""
    emu = IonEmu()
    width, view_height = ION_VIEW
    rows = ION_SURFACE_ROWS
    words = [ion_background_word(x, y) for y in range(rows) for x in range(width)]
    dest, dest_buffer = ion_byte_surface(emu, width, rows, 2, struct.pack(f'<{len(words)}H', *words))
    z_words = [ion_z_word(x, y, z_origin) for y in range(rows) for x in range(width)]
    z_surface, z_buffer = ion_byte_surface(emu, width, rows, 2,
                                           struct.pack(f'<{len(z_words)}H', *z_words))
    zbuffer = emu.alloc(0x40)
    emu.write32(zbuffer + 0x10, 0)
    emu.write32(zbuffer + 0x14, z_surface)
    emu.write32(zbuffer + 0x1C, z_buffer + 2 * len(z_words))
    emu.write32(zbuffer + 0x20, 2 * len(z_words))
    emu.write32(zbuffer + 0x24, 0x8000)
    emu.write32(zbuffer + 0x28, width)
    emu.write32(ZBUFFER_PTR, zbuffer)
    emu.write32(TEMP_SURFACE, dest)
    tactical = emu.alloc(0x1000)
    emu.write32(tactical + 0xB0, view_origin[0] & 0xFFFFFFFF)
    emu.write32(tactical + 0xB4, view_origin[1] & 0xFFFFFFFF)
    emu.write32(TACTICAL_PTR, tactical)
    emu.uc.mem_write(TACTICAL_VIEW, struct.pack('<iiii', 0, 0, width, view_height))
    emu.uc.mem_write(TACTICAL_CACHED_RECT, struct.pack('<iiii', 0, 0, width, view_height))
    emu.uc.mem_write(ADJUST_FOR_Z_SCALE, struct.pack('<Q', STANDARD_Z_MULTIPLIER_BITS))
    emu.write32(DETAIL_LEVEL, detail)
    ripple_width, ripple_height = ION_RIPPLE_SIZE
    for n in sorted({frame for *_, frame in blasts}):
        data = bytes(ion_ripple_pattern(n, sx, sy) for sy in range(ripple_height)
                     for sx in range(ripple_width))
        surface, _ = ion_byte_surface(emu, ripple_width, ripple_height, 1, data)
        emu.write32(ION_RIPPLE_SURFACES + 4 * n, surface)
    pointers = ion_vector(emu, [(x, y, z, frame, 1) for x, y, z, frame in blasts])
    points = []
    out = emu.alloc(8)
    for blast in pointers:
        emu.invoke(0x6D2140, ecx=tactical, args=[blast, out])
        points.append([*struct.unpack('<ii', emu.uc.mem_read(out, 8)),
                       emu.uc.reg_read(UC_X86_REG_EAX) & 0xFF])
    invoke_under(emu, 0x53D850, count=50_000_000, timeout_us=300_000_000)
    after = struct.unpack(f'<{len(words)}H', emu.uc.mem_read(dest_buffer, 2 * len(words)))
    changed = [[i % width, i // width, old, new]
               for i, (old, new) in enumerate(zip(words, after)) if old != new]
    if bytes(emu.uc.mem_read(z_buffer, 2 * len(z_words))) != struct.pack(f'<{len(z_words)}H',
                                                                         *z_words):
        raise OracleError('the ripple wrote the Z buffer')
    return dict(name=name, blasts=[list(blast) for blast in blasts],
                view_origin=list(view_origin), z_origin=z_origin, detail=detail,
                points=points, changed=len(changed),
                fnv1a64=f'0x{fnv1a64_bytes(struct.pack(f"<{len(after)}H", *after)):016x}',
                first_changed=changed[:16])


def ion_blast_coord(point, z, view_origin=(-96, 2940)):
    """A coordinate whose blast projects near `point` for `view_origin`
    (CoordsToClient2 is run natively on it; this only picks inputs)."""
    dx = point[0] - (-view_origin[0])
    dy = point[1] + view_origin[1] - 3000
    ax, ay = dx * 256 // 30, dy * 256 // 15
    return (ION_BASE_COORD[0] + (ax + ay) // 2, ION_BASE_COORD[1] + (ay - ax) // 2, z)


def ion_blast_draw():
    """Rows of ion_draw_row: the blast rect clipped on each side, a raised
    blast (its AdjustForZ in the threshold), two blasts drawn last first, a
    detail level below 2 and a centre left of the view."""
    origin = 0x8000 - 3
    row = ion_draw_row
    rows = [
        row(name='centre', blasts=[(*ion_blast_coord((96, 60), 0), 5)], view_origin=(-96, 2940),
            z_origin=origin),
        row(name='top_left', blasts=[(*ion_blast_coord((-150, -90), 0), 1)],
            view_origin=(-96, 2940), z_origin=origin),
        row(name='bottom_right', blasts=[(*ion_blast_coord((250, 200), 0), 35)],
            view_origin=(-96, 2940), z_origin=origin),
        row(name='left_of_view', blasts=[(*ion_blast_coord((-200, 40), 0), 12)],
            view_origin=(-96, 2940), z_origin=origin),
        row(name='two_blasts', blasts=[(*ion_blast_coord((80, 50), 0), 3),
                                       (*ion_blast_coord((110, 70), 0), 9)],
            view_origin=(-96, 2940), z_origin=origin),
        row(name='low_detail', blasts=[(*ion_blast_coord((96, 60), 0), 5)],
            view_origin=(-96, 2940), z_origin=origin, detail=1),
    ]
    for z, frame in ((1500, 20), (300, 27)):
        emu = Emu()
        emu.uc.mem_write(ADJUST_FOR_Z_SCALE, struct.pack('<Q', STANDARD_Z_MULTIPLIER_BITS))
        lift = i32(invoke_under(emu, 0x6D20E0, ecx=z))
        rows.append(row(name=f'raised_{z}', blasts=[(*ion_blast_coord((96, 60 + lift), z), frame)],
                        view_origin=(-96, 2940), z_origin=(origin - lift) & 0xFFFF))
    return rows


def generate():
    return {'source': 'unicorn/gamemd.exe', 'click_fire': click_fire(),
            'defense_alert': defense_alert(), 'mission_missile': mission_missile(),
            'nuke_maker': nuke_maker(), 'super_anim': super_anim(),
            'opening_super_anim': opening_super_anim(),
            'ai_try_fire': try_fire(), 'ai_best_rally_target': best_rally_target(),
            'ai_ground_rally_point': ground_rally_point(),
            'ai_genetic_mutator': gen_mutator(),
            'ai_psydom': psydom_ai(),
            'chrono_process': chrono_process(),
            'chrono_update_position': update_position(),
            'chrono_destination': chrono_destination(),
            'psydom_process': psydom_process(),
            'psydom_start': psydom_start(),
            'update_lighting': update_lighting(),
            'ambient_step': ambient_step(),
            'dominator_lighting_read': dominator_lighting_read(),
            'relight': relight(),
            'spy_plane_launch': spy_plane_launch(),
            'send_spy_planes': send_spy_planes(),
            'spyplane_missions': spyplane_missions(),
            'aircraft_leave_map': aircraft_leave_map(),
            'team_super_actions': team_super_actions(),
            'iron_tint': iron_tint(),
            'effect_tint_intensity': effect_tint_intensity(),
            'curtain_draw_arm': curtain_draw_arm(),
            'drawshp_curtain_arm': drawshp_curtain_arm(),
            'draw_curtain_arm': draw_curtain_arm(),
            'aircraft_light': aircraft_light(),
            'building_colour_word': building_colour_word(),
            'anim_colour_word': anim_colour_word(),
            'building_anim_light': building_anim_light(),
            'blit_pickers': blit_pickers(),
            'blitters': blitters(),
            'nuke_impact': nuke_impact(),
            'nuke_wait': nuke_wait(),
            'nuke_flash': nuke_flash(),
            'nuke_lighting_read': nuke_lighting_read(),
            'force_shield_launch': force_shield_launch(),
            'super_fade': super_fade(),
            'storm_start': storm_start(),
            'storm_cloud': storm_cloud(),
            'storm_pixel_heights': storm_pixel_heights(),
            'storm_strike': storm_strike(),
            'storm_process': storm_process(),
            'radar_outage': radar_outage(),
            'iron_curtain_launch': iron_curtain_launch(),
            'curtain_overrides': curtain_overrides(),
            'paradrop_launch': paradrop_launch(),
            'send_paradrop_planes': send_paradrop_planes(),
            'paradrop_missions': paradrop_missions(),
            'drop_payload': drop_payload(),
            'spawn_parachuted': spawn_parachuted(),
            'genetic_launch': genetic_launch(),
            'infantry_mutate_death': infantry_mutate_death(),
            'make_infantry': make_infantry(),
            'psychic_launch': psychic_launch(),
            'nuke_launch': nuke_launch(),
            'tactical_timers': tactical_timers(),
            'timer_lines': timer_lines(),
            'ion_blast_ripple': ion_blast_ripple(),
            'ion_blast_update': ion_blast_update(),
            'ion_blast_draw': ion_blast_draw(),
            'ai_catalog': {'types': [[name, what, keys] for name, what, keys in TYPE_CATALOG],
                           'build_const': BUILD_CONST_TYPES, 'build_tech': BUILD_TECH_TYPES,
                           'playfield': PLAYFIELD}}


if __name__ == '__main__':
    here = Path(__file__)
    finish_vectors(generate, here.with_suffix('.json'), source_paths={
        'superweapon_oracle': here, 'ai_base_building_oracle':
            here.with_name('ai_base_building_oracle.py')},
        provenance=lambda: provenance(
        scope=('nuclear missile launch chain: ClickFire admission/refusal/recharge writes, '
               'the computer launch alert (distance, draw, stores), Mission_Missile by '
               'status with its anims, bullet and velocity bits, NukeMaker\'s payload '
               'bullet and velocity bits, and both SuperAnim blocks; the computer\'s '
               'superweapon use: AI_TryFireSW\'s gates, arms and Force Shield timing, '
               'AI_FindBestRallyTarget\'s values, draws and pick, AI_GroundRallyPoint\'s '
               'cell, and AI_Fire_GenMutator\'s and AI_Fire_PsyDom\'s counts and picks; '
               'the Chrono Warp\'s Teleport '
               'states, owner bytes, timers and end frame, unblocked, blocked once or '
               'twice and with a stale ChronoDelay; Update_Position\'s placement, '
               'kills, blocks and blocked retarget; Launch case 4\'s destinations; the '
               'Psychic Dominator\'s ClickFire refusal, Process statuses and firing '
               'stages, Start\'s writes, UpdateLighting\'s targets and RecalcLighting '
               'arguments, the ambient fade\'s intervals, clamp and step, the map\'s '
               'Dominator lighting defaults and conversions, and the Ground/Level a '
               'full cell relight adds in each lighting state; the Spy Plane\'s launch '
               'case, SendSpyPlanes\' calls and writes, both Spy Plane missions\' '
               'branches, reveals, sound, queued missions, destinations and frames, '
               'and the aircraft off-map removal and its predicate; script actions '
               '55 and 57: the leader, the Super checked, the power and charge gates, '
               'the threat call\'s arguments, the Fire_SW indexes and cells, the '
               'mission target and the step; the Iron Curtain\'s tint stage: '
               'IronCurtain\'s writes and UpdateIronTint\'s stages, timers and '
               'Scenario draws over a curtain\'s life, GetEffectTintIntensity over '
               'every stage and time left, and the intensity and colour word '
               'DrawVoxelBody\'s curtain block hands the composite blit; the curtain\'s '
               'tint on buildings: DrawSHP\'s intensity arm for a building and a unit, '
               'the colour word DrawBody, BuildingClass::Draw and AnimClass::DrawIt '
               'compute, the building anims\' light UpdateAnimation writes, the blitter '
               'each draw\'s flags pick and whether its tinted copy ORs the word; the NUKE '
               'warhead\'s impact: '
               'its warhead test, ground clamp, flash, radar event, NUKEBALL '
               'arguments, holder list and committed cell, the wait at the AI\'s '
               'head, the flash\'s statuses, timers and relights, and the map\'s '
               'NukeAmbientChangeRate; the Force Shield\'s launch: its gate, anim, '
               'countdown and coordinate, StartSound, blackout, the buildings its walk '
               'shields by owner, distance and radius, its sentinels and the player\'s '
               'tail, and the countdown\'s SpecialSound frame; the Lightning Storm: '
               'Start\'s retarget, countdown, empty cell, radar event and outages, '
               'CreateCloudBolt\'s coordinate and draw, 0x6D2120 over pixel counts, '
               'GroundStrike\'s bolt, sound, explosion, flash, damage and debris, '
               'Process\'s lists, end, countdown, cadences and scatter, and the radar '
               'outage\'s expiry and availability test; the Iron Curtain\'s launch: '
               'its gate, anim, EVA, radar event, the 3x3 walk\'s lists, skips and '
               'IronCurtain calls and the player\'s tail, and the Infantry and Foot '
               'IronCurtain overrides\' damage, parasite release, timers and Techno '
               'call; the paradrops: Launch cases 5 and 6\'s gate, plane lookup, '
               'target and WaterSet retarget, lists by side, sends and the player\'s '
               'tail, SendParadropPlanes\' calls, edge, passengers and cargo order, '
               'both paradrop missions\' branches, passes, latch, destinations and '
               'drops, Drop_Payload\'s landing point, Can_Enter_Cell, spot, sound, '
               'neighbour cell, team, passes, rearm timer and refusal, and '
               'SpawnParachuted\'s missions and action; the Genetic Mutator: Launch '
               'case 9\'s gate, anim, EVA, sound, radar event, area damage and its '
               '3x3 walk\'s lists, WhatAmI test, Strength damage and next-before-call '
               'cursor, and the player\'s tail; ReceiveDamage\'s InfDeath 9 arm: '
               'the occupancy unmark and mark, the building, Foot-cost, bridge and '
               'spot tests, the Die2 fallback and the InfantryMutate anim with its '
               'house and remap; and AnimClass::AI\'s MakeInfantry block: the clear, '
               'the AnimToInfantry bound, the Civilian-house fallback, CreateObject, '
               'Unlimbo and its retry, the bridge lift and Hunt; the Psychic Reveal: '
               'Launch case 11\'s gate, coordinate, RevealArea arguments, sound and '
               'the player\'s tail; the nuclear missile: Launch case 0\'s gate, silo '
               'type scan and lookup, mission, house cell, firing type, sound, EVA, '
               'the player\'s tail and the recheck flag; the tactical timers: '
               'TacticalClass::Draw\'s Scenario, Super and blackout lines (index, scheme, '
               'seconds, label, blink pointers and the GameMode 0 hold skip) and each '
               'line\'s texts, measures, right alignment, black boxes, colours and blink; '
               'the IonBlast ripple: the ring step under both control words, the '
               '80 generated frames\' digests, counts and bytes, both spiral functions, '
               'UpdateAll\'s frames and removals, and DrawAll\'s pixels for clipped, '
               'raised, paired and low-detail blasts'),
        assumptions=['fresh emulator per case; fixture Super/House/Building/Bullet layouts '
                     'from live disassembly',
                     'x87 control word 0x0E7F (53-bit chop) at each entry',
                     'ClickFire: no charge drain, no one-time grant, CustomChargeTime -1',
                     'the opening block starts with EDI -1, as OnConstructionComplete '
                     'sets it at 0x445FCB'],
        substitutions=['Launch 0x6CC390, LightningStorm::HasDeferment 0x53A0E0 and '
                       'PrintMessage 0x53AE00, PsyDom::Active 0x53B400 (the row\'s answer) and '
                       'PrintMessage 0x53B410 are recorded stubs',
                       'MapClass::operator[] 0x5657A0 answers one fixture cell; its GetCoords '
                       'vt+0x48 answers the centre raised 104 leptons per supplied level',
                       'object GetCoords vt+0x48, the silo GetFLH vt+0xB0 and the yard '
                       'GetCoords answer supplied coordinates',
                       'Begin_Mode 0x447780, Queue_Mission vt+0x1E8, the anim constructor '
                       '0x421EA0 and its setters 0x424C90/0x424CA0, CreateBullet 0x46B050, '
                       'SetWeaponType 0x46B260, CoCreateInstance, BulletClass::Construct '
                       '0x4664C0, Limbo vt+0xD4, Fire vt+0x1F0 and the bullet/anim deletes '
                       'are recorded stubs; the type finders 0x427CB0 and 0x773030 answer '
                       'fixture indexes',
                       'RandomRanged 0x65C7E0 answers from the row; MissionControl 0x5B3A00 '
                       'answers a one-minute Rate row',
                       'PlayAnim 0x451890, GetCurrentMission vt+0x184 and the occupant '
                       'count vt+0x408 are recorded/supplied stubs',
                       'AI: Fire_SW 0x4FAE50, AI_GroundRallyPoint 0x509CD0, AI_Fire_PsyDom '
                       '0x50A150 and AI_Fire_GenMutator 0x509F60 are recorded stubs in '
                       'ai_try_fire, where AI_FindBestRallyTarget 0x50CBF0 answers the row\'s '
                       'cell; Find_Nearby_Passable_Cell 0x56DC20 answers the row\'s cell',
                       'AI: object WhatAmI vt+0x2C, GetCoords vt+0x48, InWhichLayer vt+0x78, '
                       'GetOwningHouse vt+0x3C, IsHighFlying vt+0x54 and GetCell vt+0x1BC '
                       'answer supplied facts; MapClass::operator[] 0x5657A0 answers fixture '
                       'cells; every IsCellInPlayfield lookup misses (level and slope 0)',
                       'ai_psydom: FootClass::Array 0x8B3DC4 holds the row\'s Feet in spawn '
                       'order, each listed in its cell as Unlimbo lists it; object '
                       'GetTechnoType vt+0x84 answers a fixture type holding the row\'s '
                       'ImmuneToPsionics (+0xD35) and BalloonHover (+0xD6A), and '
                       'IsIronCurtained vt+0x160 the row\'s fact; Fire_SW 0x4FAE50 is a '
                       'recorded stub',
                       'chrono_process: Update_Position 0x718260 is a recorded stub that '
                       'answers blocked for the row\'s first calls (moving +0x288 one cell '
                       'east) and otherwise true, setting Marked when placing; the owner\'s '
                       'vtable calls, the anim constructor 0x421EA0, VocClass::PlayAt '
                       '0x7509E0, PostWarpValidation 0x7187A0, Set_ArchiveTarget 0x70C610 and '
                       'ShortenPassiveScanTimer 0x70F770 are recorded stubs; '
                       'Passive_Target_Acquire 0x709480 answers false and IsCellInPlayfield '
                       '0x578460 true; the frame driver reads +0x270/+0x271/+0x27C for the '
                       'class AI gates (no Temporal attacker, the Unit reaches FootClass::AI)',
                       'chrono_update_position: MapClass::operator[] by coordinate 0x565730 '
                       'and by cell 0x5657A0 answer fixture cells (GetCoords: the centre raised '
                       '104 leptons per level); object IsIronCurtained vt+0x160, WhatAmI '
                       'vt+0x2C, GetCoords vt+0x48, GetTechnoType vt+0x84, ReceiveDamage '
                       'vt+0x16C and the owner\'s PUT/REMOVE vt+0xF0/vt+0xF4 are supplied or '
                       'recorded stubs; the floor height 0x578080, the zone 0x56D230 (7) and '
                       'Find_Nearby_Passable_Cell 0x56DC20 answer the row; the bridge height '
                       '0xB0EC2C holds 416, as its initializer 0x717F60 leaves it',
                       'chrono_destination: the slices run on a fixture Launch frame (the '
                       'offset entry, the clicked cell and the Super in their stack slots); '
                       'object WhatAmI vt+0x2C and GetCoords vt+0x48 answer the row; the '
                       'bridge height 0xB0C07C holds 416, as its initializer 0x6CAD80 '
                       'leaves it',
                       'psydom_process: the anim type\'s GetImage vt+0x9C answers a fixture '
                       'image whose frame count (+6) the row writes; MindControlArea '
                       '0x53B080 and UpdateLighting 0x53C280 are recorded stubs; the firing '
                       'stages come from a binary search over 0..2*frames, the test being '
                       'monotonic in the stage',
                       'psydom_start: MapClass::operator[] 0x5657A0 answers one fixture cell '
                       '(GetCoords: the centre raised 104 leptons per level); the anim '
                       'constructor 0x421EA0 and UpdateLighting 0x53C280 are recorded stubs',
                       'update_lighting: RecalcLighting 0x53AD00 is a recorded stub',
                       'ambient_step: the slice starts with EBP, EBX and EDI holding the '
                       'Scenario, the Rules and the frame, as PerTickUpdate leaves them; '
                       '0x4AE4C0 and 0x4F42F0 are recorded stubs',
                       'dominator_lighting_read: Set_Defaults runs from 0x683915 with EBX '
                       'zero and EAX 100, the values its earlier instructions leave; '
                       'CCINIClass::ReadDouble 0x5283D0 is not run: a trampoline loads its '
                       'answer (the default, or the token scanned as a float and widened) '
                       'into ST0 before each conversion slice',
                       'relight: the slice starts after the light gather with EBX and '
                       '[ESP+0x50] pointing at the top (holding the ambient) and bottom '
                       'scalars, [ESP+0x44] at the gathered additive and EDI at a fixture '
                       'cell holding only its level; the '
                       'Scenario comes from Set_Defaults\' block as in '
                       'dominator_lighting_read, with authored Ground/Level stand-ins',
                       'spy_plane_launch: AircraftTypeClass::FindIndex 0x41CAA0 answers '
                       'the row\'s index; MapClass::operator[] 0x5657A0 answers NULL, the '
                       'dummy 0xABDC50 or a fixture cell; SendSpyPlanes 0x65EAB0 and the '
                       'EVA calls 0x753250/0x752A40 are recorded stubs',
                       'send_spy_planes: CreateObject (type vt+0x8C) answers a fixture '
                       'plane or NULL; PickCellOnEdge 0x4AA440 answers the row\'s cell '
                       '(its draws are tools/spatial_oracle/aircraft_states.py\'s); the '
                       'plane\'s Queue_Mission, Assign_Destination, SetTarget, Unlimbo '
                       '(answering the row), vt+0x1EC and the delete are recorded stubs',
                       'spyplane_missions: Distance_To 0x5F6440 answers the row\'s '
                       'distance, and 0 for a NULL target as its head does; GetWeapon '
                       'vt+0x3F8 a fixture weapon with the row\'s Range and Damage; '
                       'GetTechnoType vt+0x84 a type with Sight=0; the reveal 0x5678E0, '
                       'the fog border 0x567DA0, VocClass::PlayAt 0x7509E0, '
                       'Queue_Mission and Assign_Destination are recorded stubs; '
                       'PickCellOnEdge answers the row\'s cell and MapClass::operator[] '
                       'a fixture cell',
                       'aircraft_leave_map: the slice runs on a fixture frame with ESI '
                       'the plane; Map Size is PLAYFIELD\'s width by 46; the team call '
                       '0x6EC300 answers the row; UnInit vt+0xF8 is a recorded stub',
                       'team_super_actions: member GetTechnoType vt+0x84 answers a type '
                       'holding the row\'s LeadershipRating (+0x5FC), WhatAmI vt+0x2C the '
                       'row\'s kind; the centre\'s and target\'s GetCoords vt+0x48 answer '
                       'the row; Greatest_Threat vt+0x3C4 answers the row\'s target or '
                       'NULL; Fire_SW 0x4FAE50 and Assign_Mission_Target 0x6E9050 are '
                       'recorded stubs',
                       'iron_tint: the Techno is a fixture holding its vtable (slot '
                       '+0x160 the native IsIronCurtained), both timers and the stage; '
                       'nothing is stubbed',
                       'effect_tint_intensity: one emulator for every row (the function '
                       'only reads); the fixture Techno holds the stage and tint timer, '
                       'its airstrike tint at stage 0 with both timers stopped; nothing '
                       'is stubbed',
                       'curtain_draw_arm: the slice starts with EBP the unit (vtable '
                       'slots +0x464 and +0x160 the native 0x70D190 and 0x41BF40) and '
                       '[ESP+0x1E0]/[ESP+0x1E4] the intensity and colour word; Rules '
                       'hold the retail [ColorAdd] and the row\'s indexes; the pixel '
                       'format 0x8205D0 is the row\'s (retail RGB565 is 2); the high half '
                       'of the colour word holds stale ECX bits from the fixture Rules '
                       'address; nothing is stubbed',
                       'drawshp_curtain_arm, draw_curtain_arm, building_colour_word, '
                       'anim_colour_word, building_anim_light: the Techno is '
                       'curtain_techno\'s, its vtable '
                       'holding the native IsIronCurtained 0x41BF40 and flash arms '
                       '(0x456F80, 0x70D190); WhatAmI vt+0x2C, GetCoords vt+0x48 and GetCell '
                       'vt+0x1BC answer the row; MapClass::operator[] 0x5657A0 and by '
                       'coordinate 0x565730 answer one fixture cell; 0x487950 (the cell\'s '
                       'centre shrouded) answers the row; the slices start with the '
                       'registers their blocks read (ESI, EBP or EAX as named); above the '
                       'low 16 bits the colour words hold stale register bits',
                       'aircraft_light: the slice starts with EBP a fixture aircraft '
                       'whose GetHeight vt+0x1C8 answers the row and [ESP+0x28] the '
                       'coordinate; the storm byte 0xA9FAB4, Scenario Level= (+0x3544) '
                       'and IonLevel= (+0x355C), Rules ExtraAircraftLight= (+0x17DC) and '
                       'one cell in Map\'s table (+0x13C, +0x140) with the row\'s word '
                       '(+0x10A) are fixtures; the level step 0x889EC8 is what the '
                       'aircraft unit\'s initializers 0x413AC0..0x413B90 leave',
                       'blit_pickers: a fixture Convert whose fields hold their offsets',
                       'blitters: the blitter objects carry the real vtables the 16-bit '
                       'Convert constructor 0x48E740 stores in each field, with fixture '
                       'conversion and row tables; the ZBuffer (0x887644) and ABuffer '
                       '(0x87E8A4) instances wrap far past the lines',
                       'nuke_impact: the slice starts with EBP the bullet (vtable slots '
                       '+0x1C8/+0x1CC/+0x1B8 the native GetHeight, SetHeight and '
                       'GetMapCoords; Mark +0x124 and UnInit +0xF8 recorded stubs), '
                       '[ESP+0x18] the impact flag and [ESP+0x24] the committed '
                       'candidate; the floor height 0x578080 answers the row\'s ground; '
                       'AnimTypeClass::Array holds the row\'s types; ScreenNukeFlash '
                       '0x53AB70, CreateRadarEvent 0x65FA70, the anim constructor '
                       '0x421EA0 and the handoff 0x468D80 are recorded stubs; the holder '
                       'list 0xB0F5B8 has room for four; the deck height 0xAC13BC holds '
                       '416',
                       'nuke_wait: ObjectClass::AI 0x5F3E70, the holder list\'s Find '
                       'vt+0x10, the handoff 0x468D80 and UnInit vt+0xF8 are recorded or '
                       'supplied stubs',
                       'nuke_flash: the Scenario holds Set_Defaults\' lighting block; the '
                       'flash starts reset as SuperWeaponEffects::ResetAll 0x539760 leaves '
                       'it; RecalcLighting 0x53AD00, UpdateLighting 0x53C280 and the redraw '
                       '0x4F42F0 are recorded stubs; status 3 has no writer but the save '
                       'stream (0x53993E)',
                       'nuke_lighting_read: as dominator_lighting_read, ReadDouble 0x5283D0 '
                       'is not run',
                       'force_shield_launch: MapClass::operator[] 0x5657A0 answers a '
                       'fixture cell whose GetCoords vt+0x48 answers the row\'s centre '
                       'raised 104 leptons per level (or the row\'s coordinate) and whose '
                       '+0x140 holds the row\'s bridge bit; the bridge height 0xB0C07C '
                       'holds 416 and the sentinels come from their static initializers '
                       '0x6CADC0/0x6CADE0; BuildingClass::Array holds fixture buildings '
                       'whose GetCoords vt+0x48 answers the row and whose IronCurtain '
                       'vt+0x154 is a recorded stub; houses hold only ArrayIndex (+0x30) '
                       'and their ally bits (+0x5788); the anim constructor 0x421EA0, '
                       'VocClass::PlayAt 0x7509E0, the blackout 0x50BC90 and the EVA calls '
                       '0x753250/0x752A40 are recorded stubs',
                       'super_fade: an ungranted Super (+0x6D clear) holding no anim '
                       '(+0x68) with no Super selected; VocClass::PlayAt 0x7509E0 is a '
                       'recorded stub',
                       'storm_*: the storm unit\'s CRT initializers 0x539220..0x539500 and '
                       'the pixel scale\'s 0x6D1830/0x6D18C0/0x6D1BF0 run first; fixture '
                       'anims hold their stage at +0xAC and answer vt+0x88 (type) and '
                       'vt+0x48 (coordinate); fixture types answer vt+0x9C with an SHP '
                       'header (+4 height, +6 frames); MapClass::operator[] 0x5657A0 and by '
                       'coordinate 0x565730 answer fixture cells (+0x24 cell, +0x11B level, '
                       '+0x140 bridge bit, +0xEC land), whose floor 0x47B3A0, building '
                       '0x47C520 and nearest object 0x47C3D0 answer the row (the damage stub '
                       'applies the row\'s after-state); In_Bounds 0x568300 runs over the '
                       'row\'s Size (+0xF4/+0xF8) beside MapRect (1, 1, W+H-1, W+H-1); the '
                       'anim constructor 0x421EA0, the selector 0x48A4F0, the '
                       'flash 0x48A620, Apply_area_damage 0x489280, PlayAt 0x7509E0, '
                       'PlayAtPos 0x750920, PlayEVA 0x752700, LoadString 0x734E60, '
                       'Add_Message 0x5D3BA0, CreateRadarEvent 0x65FA70, UpdateLighting '
                       '0x53C280, the redraw 0x4F42F0, PsychicDominator::Process 0x53AF40 '
                       'and 0x53B560 are recorded or silent stubs; the three lists have room '
                       'for 256',
                       'radar_outage: the expiry slice starts with ESI the house; 0x508DF0 '
                       'runs for PlayerPtr with Scenario+0x34A4 set and 0x656DE0 answering '
                       'neither value, 0x656DF0 recorded',
                       'iron_curtain_launch: the offset table\'s initializer 0x6CAE00 runs '
                       'first and the bridge height 0xB0C07C holds 416; MapClass::operator[] '
                       '0x5657A0 answers one fixture cell per coordinate (GetCoords vt+0x48 the '
                       'centre raised 104 leptons per level, +0x140 the row\'s bridge bit, '
                       '+0xE4/+0xE8 the row\'s lists linked through +0x30); each object\'s '
                       'IronCurtain vt+0x154 is a recorded stub, clearing its +0x30 for an '
                       'unlink object; the anim constructor 0x421EA0, PlayEVA 0x752700, '
                       'CreateRadarEvent 0x65FA70 and the EVA calls 0x753250/0x752A40 are '
                       'recorded stubs; 0xA8B538 holds the row\'s mute byte',
                       'curtain_overrides: the fixture Techno\'s type (+0x6C0, and GetTechnoType '
                       'vt+0x84) holds Strength (+0xA0) and Organic (+0xD97); ReceiveDamage '
                       'vt+0x16C, ExitUnit 0x62A4A0 and TechnoClass::IronCurtain 0x70E2B0 are '
                       'recorded stubs; the timers\' middle word, a stale local, is not '
                       'recorded',
                       'paradrop_launch: the empty cell\'s initializer 0x6CADB0 runs first and '
                       'WaterSet 0xAA0738 holds 50; MapClass::operator[] 0x5657A0 answers each '
                       'row cell (null, the dummy 0xABDC50, or a fixture cell whose +0x38 tile '
                       'is WaterSet plus an offset); each list entry is its own fixture '
                       'InfantryTypeClass (+0xDF8 ArrayIndex) and each nums vector is followed '
                       'by 0x5EED; the PDPLANE lookup 0x41CAA0, Find_Nearby_Passable_Cell '
                       '0x56DC20, SendParadropPlanes 0x65E660 and the EVA calls are recorded '
                       'stubs',
                       'send_paradrop_planes: AircraftTypeClass::Array 0xA8B21C and '
                       'InfantryTypeClass::Array 0xA8E34C hold one fixture type each, whose '
                       'CreateObject vt+0x8C is a recorded stub; PickCellOnEdge 0x4AA440 and '
                       'the plane\'s and infantry\'s virtuals are recorded stubs; the fixture '
                       'infantry carry AbstractFlags 4; a negative num, which counts down '
                       'through zero, is not run',
                       'paradrop_missions: Distance_To 0x5F6440 answers the row\'s distance; '
                       'IsCoordInPlayfield 0x5785F0, Drop_Payload 0x415C60 and the plane\'s '
                       'virtuals are recorded stubs; the first passenger +0x118 is a '
                       'fixture pointer or NULL',
                       'drop_payload: the plane\'s GetCoords vt+0x48 answers its +0x9C and '
                       'FacingClass::Current 0x4C93D0 the row\'s facing; GetCellAt 0x565730 '
                       'answers one fixture cell whose GetCoords answers (40, 41)\'s centre; '
                       '0x481180 answers its request moved (+21, -13) at Z 7, or the empty '
                       'coordinate; the passengers\' +0x55C start at (-3, -3); the rearm '
                       'timer\'s middle word, a stale local, is not recorded',
                       'spawn_parachuted: ObjectClass::Paradrop 0x5F5940, Queue_Mission '
                       'vt+0x1E8 and Do_Action vt+0x558 are recorded stubs; the owner\'s '
                       '+0x1EC/+0x1ED and SessionClass::GameMode 0xA8B238 are the row\'s',
                       'genetic_launch: MapClass::operator[] 0x5657A0 answers one fixture '
                       'cell per coordinate (GetCoords the centre raised 104 leptons per '
                       'level, +0x140 the bridge bit, the row\'s objects on +0xE4/+0xE8); '
                       'each object\'s WhatAmI vt+0x2C answers the row, GetTechnoType vt+0x84 '
                       'a fixture type holding its Strength; the anim constructor, PlayEVA, '
                       'PlayAtCoord 0x750E20, CreateRadarEvent, Apply_area_damage 0x489280, '
                       'ReceiveDamage vt+0x16C and the EVA queue calls are recorded stubs',
                       'infantry_mutate_death: run as a slice on a fixture frame whose '
                       '[ESP+0xE0]/[ESP+0xEC] hold the source and house arguments; '
                       'MapClass::operator[] and GetCellAt 0x565730 answer one fixture cell '
                       '(LandType 3, its Ground-table Foot cost written at 0x89EA40); '
                       '0x481180 answers its request moved (+21, -13) at Z 7, or the empty '
                       'coordinate; Mark/Unmark, GetCoords, the anim constructor and the '
                       'anim\'s MarkCellOccupancy are recorded stubs; ColorScheme::Array '
                       '0xB054D4 holds fixture schemes whose +0x30C is 0xC0DE00 plus their '
                       'index. With neither a source nor a house the arm writes no remap; '
                       'the 0 recorded then is the stubbed constructor\'s',
                       'make_infantry: run as a slice (EAX the anim type, EBP -1, EDI 0); '
                       'AnimToInfantry\'s items sit inside a fixture vector readable 16 '
                       'entries either side, each its own InfantryType whose ArrayIndex '
                       '(+0xDF8) is 16 past its slot; SideClass::Find_Index 0x6A46D0 answers '
                       'the row\'s Civilian side; the anim\'s, type\'s, infantry\'s and '
                       'cell\'s virtuals are recorded stubs; the cell\'s GetCoords answers '
                       '(40, 41)\'s centre at Z 208',
                       'psychic_launch: MapClass::operator[] 0x5657A0 answers one fixture '
                       'cell (GetCoords the centre raised 104 leptons per level, +0x140 the '
                       'bridge bit); MapClass::RevealArea 0x5678E0, PlayAtCoord 0x750E20 and '
                       'the EVA queue calls are recorded stubs',
                       'nuke_launch: the Super is not one-time (+0x6E clear); '
                       'BuildingTypeClass::Array 0xA83C6C/0xA83C78 holds the row\'s fixture '
                       'types (+0x16BA, +0x16F0, +0x16F4); Find_Building_Of_Type 0x4FD060, the '
                       'floor height 0x578080, the building\'s vt+0x1E8/vt+0x1EC, PlayAtCoord '
                       '0x750E20, PlayEVA 0x752700 and the EVA queue calls are recorded '
                       'stubs; 0xA8B538 holds the row\'s mute byte',
                       'tactical_timers: the slice starts at 0x6D4941 on a fresh stack; '
                       'houses come from the fixture heap; the string table 0x734E60 and '
                       'the line draw 0x6D4B50 are recorded stubs',
                       'timer_lines: swprintf 0x7CA564 and vswprintf 0x7CA858 format in '
                       'Python; the measure 0x433CF0 answers timer_glyph_width and the font '
                       'height; the clock 0x4093B0 answers the row\'s milliseconds; the '
                       'Composite surface\'s GetRect vt+0x78 answers the view and FillRect '
                       'vt+0x14 and the glyph print 0x434B90 are recorded stubs; the pixel '
                       'format is RGB565',
                       'ion_blast_ripple: operator new 0x7C8E17 takes a 16 MiB fixture heap; '
                       'the BSurface constructor, Fill and Lock run natively; the ring '
                       'step\'s initializers run under 0x027F (the C++ initializers run '
                       'before WinMain sets truncation at 0x6BBFC1) before the generator, '
                       'which runs under 0x0E7F',
                       'ion_blast_update: the vector\'s initializer 0x53CA70 runs first; '
                       'the fixture items are not allocated (+0xD clear); operator delete '
                       '0x7C8B3D is a silent stub',
                       'ion_blast_draw: the destination (TempSurface) and the Z buffer '
                       'surface are 192x144 two-byte BSurfaces (vtable 0x7E2070), the '
                       'tactical view and CoordsToClient2\'s rect are 192x128 at the '
                       'origin, the ZBuffer has offset 0, +0x24 0x8000 and +0x28 192, '
                       'the AdjustForZ scale 0xB0CD48 holds its startup value, and the '
                       'ripple frames are byte BSurfaces holding ion_ripple_pattern'],
        entry_points={'ClickFire': 0x6CB920, 'defense_alert': 0x4FAF00,
                      'Mission_Missile': 0x44C980, 'NukeMaker': 0x46B310,
                      'UpdateAnimation_super_anim': 0x450F9E,
                      'OnConstructionComplete_super_anim': 0x4463F0,
                      'AI_TryFireSW': 0x5098F0, 'AI_FindBestRallyTarget': 0x50CBF0,
                      'AI_GroundRallyPoint': 0x509CD0, 'AI_Fire_GenMutator': 0x509F60,
                      'AI_Fire_PsyDom': 0x50A150,
                      'TeleportLocomotionClass::Process': 0x7192F0,
                      'TeleportLocomotionClass::Update_Position': 0x718260,
                      'SuperClass::Launch_case4_destination': 0x6CC9AF,
                      'PsychicDominator::Process': 0x53AF40, 'PsyDom::Start': 0x53AE50,
                      'ScenarioClass::UpdateLighting': 0x53C280,
                      'LogicClass::PerTickUpdate_ambient_fade': 0x55B33D,
                      'ScenarioClass::Set_Defaults_lighting': 0x683915,
                      'ScenarioClass::Read_INI_Basic_dominator': 0x68AAFD,
                      'CellClass::ProcessColourComponents_profile_arms': 0x48445F,
                      'SuperClass::Launch_case8': 0x6CC390,
                      'HouseClass::SendSpyPlanes': 0x65EAB0,
                      'AircraftClass::Mission_SpyplaneApproach': 0x4155F0,
                      'AircraftClass::Mission_SpyplaneOverfly': 0x4157C0,
                      'AircraftClass::AI_leave_map': 0x414F47,
                      'TeamClass::script_action_55': 0x6EFC70,
                      'TeamClass::script_action_57': 0x6F0130,
                      'TechnoClass::IronCurtain': 0x70E2B0,
                      'TechnoClass::UpdateIronTint': 0x70E5A0,
                      'TechnoClass::GetEffectTintIntensity': 0x70E360,
                      'UnitClass::DrawVoxelBody_curtain_block': 0x73BF7B,
                      'TechnoClass::DrawSHP_curtain_arm': 0x70631F,
                      'TechnoClass::Draw_curtain_arm': 0x706776,
                      'AircraftClass::Draw_It_light': 0x4148D1,
                      'AircraftClass level step initializer': 0x413B90,
                      'BuildingClass_DrawBody_colour_word': 0x43D386,
                      'BuildingClass::Draw_colour_word': 0x43DC1C,
                      'AnimClass::DrawIt_colour_word': 0x4233EE,
                      'BuildingClass::UpdateAnimation_anim_light': 0x450A47,
                      'ConvertClass_pick_blitter': 0x490B90,
                      'ConvertClass_pick_compressed_blitter': 0x490E50,
                      'BulletClass::AI_nuke_impact': 0x467E53,
                      'BulletClass::AI_head': 0x4666F2,
                      'ScreenNukeFlash': 0x53AB70,
                      'LightningStorm::Process_nuke_flash': 0x53A6C0,
                      'ScenarioClass::Read_INI_Basic_nuke_rate': 0x68AAD5,
                      'SuperClass::Launch_case10': 0x6CC390,
                      'SuperClass::AI_fade': 0x6CBCA0,
                      'LightningStorm::Start': 0x539EB0,
                      'LightningStorm::CreateCloudBolt': 0x53A140,
                      'LightningStorm::GroundStrike': 0x53A300,
                      'LightningStorm::Process': 0x53A6C0,
                      'pixel_height': 0x6D2120,
                      'HouseClass::Update_radar_outage': 0x4F8490,
                      'HouseClass::radar_availability': 0x508DF0,
                      'SuperClass::Launch_case1': 0x6CC390,
                      'InfantryClass::IronCurtain': 0x522600,
                      'FootClass::IronCurtain': 0x4DEAE0,
                      'SuperClass::Launch_cases5_6': 0x6CC390,
                      'HouseClass::SendParadropPlanes': 0x65E660,
                      'AircraftClass::Mission_ParadropApproach': 0x4158E0,
                      'AircraftClass::Mission_ParadropOverfly': 0x415960,
                      'AircraftClass::Drop_Payload': 0x415C60,
                      'InfantryClass::SpawnParachuted': 0x521760,
                      'SuperClass::Launch_case9': 0x6CC390,
                      'InfantryClass::ReceiveDamage_infdeath9': 0x5188AE,
                      'AnimClass::AI_make_infantry': 0x424932,
                      'SuperClass::Launch_case11': 0x6CC390,
                      'SuperClass::Launch_case0': 0x6CC390,
                      'TacticalClass::Draw_timers': 0x6D4941,
                      'TacticalClass::DrawTimer': 0x6D4B50,
                      'IonBlast ring step initializers': 0x53CA30,
                      'IonBlast ripple generator': 0x53D330,
                      'IonBlast spiral point': 0x53D8E0,
                      'IonBlast spiral index': 0x53D960,
                      'IonBlastClass::UpdateAll': 0x53D310,
                      'IonBlastClass::DrawAll': 0x53D850}))
